
#include "Precomp.h"
#include "NHarryPotter.h"
#include "VM/NativeFunc.h"
#include "Engine.h"
#include "Packages/Engine/Actors/UActor.h"
#include "Packages/Engine/Actors/Pawn/UPawn.h"
#include "Packages/Engine/Actors/NavigationPoint/UNavigationPoint.h"
#include "Packages/Engine/Actors/Info/ULevelInfo.h"
#include "Packages/Engine/Resources/Level/ULevel.h"
#include "Packages/Engine/Resources/USound.h"
#include "Packages/Engine/Subsystems/USurrealAudioDevice.h"
#include <queue>
#include <map>

void NHarryPotter::RegisterFunctions()
{
	RegisterVMNativeFunc_3("Gesture", "CompareGesture", &NHarryPotter::CompareGesture, 426);
	RegisterVMNativeFunc_3("Gesture", "CompareGesturePoint", &NHarryPotter::CompareGesturePoint, 427);
	RegisterVMNativeFunc_2("Wind", "GetWind", &NHarryPotter::GetWind, 425);
	RegisterVMNativeFunc_3("Pawn", "FindPath", &NHarryPotter::FindPath, 553);
	RegisterVMNativeFunc_5("Actor", "ModifySound", &NHarryPotter::ModifySound, 567);
	RegisterVMNativeFunc_1("Actor", "SaveGameExists", &NHarryPotter::SaveGameExists, 3972);
}

/////////////////////////////////////////////////////////////////////////////
// Gesture: a 2D pattern (a polyline in the unit square) that the player traces with the wand in spell lessons

static ScriptArray* GetGesturePoints(UObject* gesture)
{
	PropertyDataOffset offset = gesture->GetPropertyDataOffset("Points");
	if (offset.DataOffset == ~(size_t)0)
		return nullptr;
	return static_cast<ScriptArray*>(gesture->PropertyData.Ptr(offset.DataOffset));
}

static float DistanceToSegment2D(const vec3& p, const vec3& a, const vec3& b)
{
	float abx = b.x - a.x, aby = b.y - a.y;
	float apx = p.x - a.x, apy = p.y - a.y;
	float len2 = abx * abx + aby * aby;
	float t = len2 > 0.0f ? clamp((apx * abx + apy * aby) / len2, 0.0f, 1.0f) : 0.0f;
	float dx = apx - abx * t, dy = apy - aby * t;
	return std::sqrt(dx * dx + dy * dy);
}

static float DistanceToGesture(const vec3* points, size_t count, const vec3& p)
{
	if (count == 0)
		return 1000.0f;
	float best = DistanceToSegment2D(p, points[0], points[0]);
	for (size_t i = 0; i + 1 < count; i++)
		best = std::min(best, DistanceToSegment2D(p, points[i], points[i + 1]));
	return best;
}

// Distance from a drawn point to the pattern. The scripts treat anything above fAccuracy as a bad point.
void NHarryPotter::CompareGesturePoint(UObject* Self, const vec3& InMousePoint, float fAccuracy, float& ReturnValue)
{
	ScriptArray* points = GetGesturePoints(Self);
	ReturnValue = points ? DistanceToGesture(static_cast<const vec3*>(points->GetData()), points->GetSize(), InMousePoint) : 1000.0f;
}

// Score from 0 to 1 for a whole drawing: how much of the pattern was traced, scaled down by the share of stray points.
// Unused entries of the drawing have Z set to -1.
void NHarryPotter::CompareGesture(UObject* Self, ScriptArray& InMousePoints, float fAccuracy, float& ReturnValue)
{
	ReturnValue = 0.0f;

	ScriptArray* gesture = GetGesturePoints(Self);
	if (!gesture || gesture->GetSize() == 0)
		return;

	const vec3* pattern = static_cast<const vec3*>(gesture->GetData());
	size_t patternCount = gesture->GetSize();

	Array<vec3> drawn;
	const vec3* mouse = static_cast<const vec3*>(InMousePoints.GetData());
	for (size_t i = 0; i < InMousePoints.GetSize(); i++)
	{
		if (mouse[i].z != -1.0f)
			drawn.push_back(mouse[i]);
	}
	if (drawn.empty())
		return;

	// Share of drawn points that are on the pattern
	size_t good = 0;
	for (const vec3& p : drawn)
	{
		if (DistanceToGesture(pattern, patternCount, p) <= fAccuracy)
			good++;
	}
	float precision = (float)good / (float)drawn.size();

	// Share of the pattern that has a drawn point nearby, sampled evenly along its length
	float totalLength = 0.0f;
	for (size_t i = 0; i + 1 < patternCount; i++)
		totalLength += length(vec3(pattern[i + 1].x - pattern[i].x, pattern[i + 1].y - pattern[i].y, 0.0f));

	const int numSamples = 100;
	int covered = 0;
	for (int s = 0; s < numSamples; s++)
	{
		// Find the sample position on the polyline
		float target = totalLength * ((float)s + 0.5f) / (float)numSamples;
		vec3 sample = pattern[0];
		float walked = 0.0f;
		for (size_t i = 0; i + 1 < patternCount; i++)
		{
			float segLength = length(vec3(pattern[i + 1].x - pattern[i].x, pattern[i + 1].y - pattern[i].y, 0.0f));
			if (walked + segLength >= target && segLength > 0.0f)
			{
				sample = mix(pattern[i], pattern[i + 1], (target - walked) / segLength);
				break;
			}
			walked += segLength;
			sample = pattern[i + 1];
		}

		for (const vec3& p : drawn)
		{
			float dx = p.x - sample.x, dy = p.y - sample.y;
			if (dx * dx + dy * dy <= fAccuracy * fAccuracy)
			{
				covered++;
				break;
			}
		}
	}
	float coverage = (float)covered / (float)numSamples;

	float score = coverage * precision;
	ReturnValue = (score >= 0.97f) ? 1.0f : score;
}

/////////////////////////////////////////////////////////////////////////////

// Wind velocity at a location. No script in the game calls this; the particle system applies wind itself.
void NHarryPotter::GetWind(UObject* Self, const vec3& Loc, vec3& ReturnValue)
{
	ReturnValue = vec3(0.0f);
}

// Next navigation point on the shortest route from startPoint to the navigation point named DestName.
// Characters use it to walk between "stations". Returns None if there is no route or the start is the destination.
void NHarryPotter::FindPath(UObject* Self, UObject* startPoint, const NameString& DestName, UObject*& ReturnValue)
{
	ReturnValue = nullptr;

	UPawn* SelfPawn = UObject::Cast<UPawn>(Self);
	ULevel* level = SelfPawn->XLevel();

	UNavigationPoint* start = UObject::TryCast<UNavigationPoint>(startPoint);
	UNavigationPoint* dest = nullptr;
	float bestStartDist = 0.0f;
	vec3 startLocation = UObject::TryCast<UActor>(startPoint) ? UObject::Cast<UActor>(startPoint)->Location() : SelfPawn->Location();

	for (UNavigationPoint* nav = SelfPawn->Level()->NavigationPointList(); nav; nav = nav->nextNavigationPoint())
	{
		if (nav->Name == DestName)
			dest = nav;

		// Not standing on a navigation point: start from the closest one
		if (!UObject::TryCast<UNavigationPoint>(startPoint))
		{
			vec3 d = nav->Location() - startLocation;
			float dist = dot(d, d);
			if (!start || dist < bestStartDist)
			{
				start = nav;
				bestStartDist = dist;
			}
		}
	}

	if (!start || !dest || start == dest)
		return;

	// Dijkstra over the level's reach specs
	struct Visit { int Cost; UNavigationPoint* Previous; };
	std::map<UNavigationPoint*, Visit> visited;
	typedef std::pair<int, UNavigationPoint*> QueueItem;
	std::priority_queue<QueueItem, std::vector<QueueItem>, std::greater<QueueItem>> queue;

	visited[start] = { 0, nullptr };
	queue.push({ 0, start });

	while (!queue.empty())
	{
		QueueItem item = queue.top();
		queue.pop();

		UNavigationPoint* node = item.second;
		if (item.first > visited[node].Cost)
			continue;
		if (node == dest)
			break;

		auto paths = node->Paths();
		for (int i = 0; i < 16; i++)
		{
			int specIndex = paths[i];
			if (specIndex < 0 || (size_t)specIndex >= level->ReachSpecs.size())
				break;

			const LevelReachSpec& spec = level->ReachSpecs[specIndex];
			UNavigationPoint* next = spec.endActor;
			if (!next || next == node)
				continue;

			int cost = item.first + std::max(spec.distance, 1) + next->ExtraCost();
			auto it = visited.find(next);
			if (it == visited.end() || cost < it->second.Cost)
			{
				visited[next] = { cost, node };
				queue.push({ cost, next });
			}
		}
	}

	auto it = visited.find(dest);
	if (it == visited.end())
		return;

	// Walk back to the step right after the start
	UNavigationPoint* step = dest;
	while (visited[step].Previous && visited[step].Previous != start)
		step = visited[step].Previous;
	ReturnValue = step;
}

// Changes volume, radius or pitch of a sound the actor is playing. Returns false if it is no longer playing.
void NHarryPotter::ModifySound(UObject* Self, uint8_t parameter, float value, std::optional<UObject*> Sound, std::optional<uint8_t> Slot, BitfieldBool& ReturnValue)
{
	UActor* SelfActor = UObject::Cast<UActor>(Self);
	USound* sound = Sound ? UObject::TryCast<USound>(*Sound) : nullptr;
	ReturnValue = engine->audiodev->ModifyActorSound(SelfActor, sound, Slot ? (int)*Slot : -1, parameter, value);
}

// To do: save games
void NHarryPotter::SaveGameExists(UObject* Self, BitfieldBool& ReturnValue)
{
	ReturnValue = false;
}
