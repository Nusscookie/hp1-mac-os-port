
#include "Precomp.h"
#include "CollisionSystem.h"
#include "Packages/Engine/Actors/UActor.h"
#include "Packages/Engine/Actors/Brush/UMover.h"
#include "Packages/Engine/Actors/Info/ULevelInfo.h"
#include "Packages/Engine/Resources/Level/UModel.h"
#include "Math/floating.h"
#include "Math/coords.h"
#include "Collision/BottomLevel/TraceRayModel.h"
#include "Collision/TopLevel/OverlapTest.h"
#include "Collision/TopLevel/TraceTest.h"
#include "Engine.h"

bool CollisionSystem::IsOverlapping(UActor* actor1, UActor* actor2)
{
	OverlapTester overlap(this);
	return overlap.IsOverlapping(actor1, actor2);
}

CollisionHitList CollisionSystem::OverlapTest(UActor* actor)
{
	return OverlapTest(actor->Location(), actor->CollisionHeight(), actor->CollisionRadius(), true, false, false);
}

CollisionHitList CollisionSystem::OverlapTest(const vec3& location, float height, float radius, bool testActors, bool testWorld, bool visibilityOnly)
{
	OverlapTester overlap(this);
	return overlap.TestOverlap(location, height, radius, testActors, testWorld, visibilityOnly);
}

bool CollisionSystem::TraceAnyHit(vec3 from, vec3 to, UActor* tracingActor, bool traceActors, bool traceWorld, bool visibilityOnly)
{
	TraceTester trace(this);
	return trace.TraceAnyHit(from, to, tracingActor, traceActors, traceWorld, visibilityOnly);
}

CollisionHitList CollisionSystem::Trace(const vec3& from, const vec3& to, float height, float radius, bool traceActors, bool traceWorld, bool visibilityOnly)
{
	TraceTester trace(this);
	return trace.Trace(from, to, height, radius, traceActors, traceWorld, visibilityOnly);
}

CollisionHitList CollisionSystem::TraceDecal(const dvec3& origin, double tmin, const dvec3& dirNormalized, double tmax, bool visibilityOnly)
{
	TraceRayModel trace;
	return trace.Trace(LevelInfo, Level->Model, origin, tmin, dirNormalized, tmax, visibilityOnly);
}

CollisionHit CollisionSystem::TraceFirstHit(const vec3& from, const vec3& to, UActor* tracingActor, const vec3& extents, const TraceFlags& flags)
{
	for (const CollisionHit& hit : Trace(from, to, extents.z, extents.x, flags.traceActors(), flags.traceWorld(), false))
	{
		if (hit.Actor != LevelInfo && (!tracingActor || !tracingActor->IsOwnedBy(hit.Actor)))
		{
			if (hit.Actor->IsA("Pawn"))
			{
				if (flags.pawns)
					return hit;
			}
			else if (hit.Actor->IsA("Mover"))
			{
				if (flags.movers)
					return hit;
			}
			else if (hit.Actor->IsA("ZoneInfo"))
			{
				if (flags.zoneChanges)
					return hit;
			}
			else if (flags.others)
			{
				if (!flags.onlyProjectiles || hit.Actor->bProjTarget() || (hit.Actor->bBlockActors() && hit.Actor->bBlockPlayers()))
					return hit;
			}
		}
		else if (flags.world && hit.Actor == LevelInfo)
		{
			return hit;
		}
	}
	return {};
}

Array<UActor*> CollisionSystem::CollidingActors(const vec3& origin, float radius)
{
	OverlapTester overlap(this);
	return overlap.CollidingActors(origin, radius);
}

Array<UActor*> CollisionSystem::CollidingActors(const vec3& origin, float height, float radius)
{
	OverlapTester overlap(this);
	return overlap.CollidingActors(origin, height, radius);
}

Array<UActor*> CollisionSystem::EncroachingActors(UActor* actor)
{
	OverlapTester overlap(this);
	return overlap.EncroachingActors(actor);
}

void CollisionSystem::SetLevel(ULevelInfo* levelInfo, ULevel* level)
{
	LevelInfo = levelInfo;
	Level = level;
}

void CollisionSystem::UpdateCollisionShape(UActor* actor)
{
	actor->Collision.IsBox = false;
	if (!engine->LaunchInfo.IsHarryPotter1() || actor->Brush())
		return;

	// Harry Potter's Actor class has CollideType and CollisionWidth. The offsets are the same for every actor.
	static bool lookedUp = false;
	static PropertyDataOffset collideTypeOffset, widthOffset;
	static bool found = false;
	if (!lookedUp)
	{
		lookedUp = true;
		found = actor->HasProperty("CollideType") && actor->HasProperty("CollisionWidth");
		if (found)
		{
			collideTypeOffset = actor->GetPropertyDataOffset("CollideType");
			widthOffset = actor->GetPropertyDataOffset("CollisionWidth");
		}
	}
	if (!found)
		return;

	enum { CT_AlignedCylinder, CT_OrientedCylinder, CT_Box, CT_Shape };
	if (actor->Value<uint8_t>(collideTypeOffset) != CT_Box)
		return;

	float width = actor->Value<float>(widthOffset);
	if (width <= 0.0f || actor->CollisionRadius() <= 0.0f)
		return;

	mat4 rotation = Coords::Rotation(actor->Rotation()).ToMatrix();
	actor->Collision.IsBox = true;
	actor->Collision.BoxCenter = actor->Location();
	actor->Collision.BoxHalfSize = vec3(actor->CollisionRadius(), width, actor->CollisionHeight());
	actor->Collision.BoxAxis[0] = normalize((rotation * vec4(1.0f, 0.0f, 0.0f, 0.0f)).xyz());
	actor->Collision.BoxAxis[1] = normalize((rotation * vec4(0.0f, 1.0f, 0.0f, 0.0f)).xyz());
	actor->Collision.BoxAxis[2] = normalize((rotation * vec4(0.0f, 0.0f, 1.0f, 0.0f)).xyz());
}

vec3 CollisionSystem::BoxExpansion(UActor* box, double cylinderHeight, double cylinderRadius)
{
	// Grow each box axis by how far a vertical cylinder reaches along it (its support distance),
	// which turns a cylinder/box test into a point/box or ray/box test.
	vec3 expansion;
	for (int i = 0; i < 3; i++)
	{
		const vec3& a = box->Collision.BoxAxis[i];
		expansion[i] = (float)(cylinderRadius * std::sqrt(a.x * a.x + a.y * a.y) + cylinderHeight * std::abs(a.z));
	}
	return expansion;
}

bool CollisionSystem::BoxPointOverlap(UActor* box, const dvec3& point, const vec3& expansion)
{
	dvec3 d = point - to_dvec3(box->Collision.BoxCenter);
	for (int i = 0; i < 3; i++)
	{
		double local = dot(d, to_dvec3(box->Collision.BoxAxis[i]));
		if (std::abs(local) >= (double)box->Collision.BoxHalfSize[i] + expansion[i])
			return false;
	}
	return true;
}

double CollisionSystem::BoxRayTrace(UActor* box, const dvec3& origin, double tmin, const dvec3& dirNormalized, double tmax, const vec3& expansion, vec3& normal)
{
	dvec3 d = origin - to_dvec3(box->Collision.BoxCenter);
	double tnear = -1e30, tfar = 1e30;
	int nearAxis = -1;
	double nearSign = 0.0;
	double localPos[3], localDir[3], half[3];
	for (int i = 0; i < 3; i++)
	{
		dvec3 axis = to_dvec3(box->Collision.BoxAxis[i]);
		localPos[i] = dot(d, axis);
		localDir[i] = dot(dirNormalized, axis);
		half[i] = (double)box->Collision.BoxHalfSize[i] + expansion[i];
		if (std::abs(localDir[i]) < 1e-9)
		{
			if (std::abs(localPos[i]) >= half[i])
				return tmax;
			continue;
		}
		double t1 = (-half[i] - localPos[i]) / localDir[i];
		double t2 = (half[i] - localPos[i]) / localDir[i];
		if (t1 > t2) std::swap(t1, t2);
		if (t1 > tnear)
		{
			tnear = t1;
			nearAxis = i;
			nearSign = localDir[i] > 0.0 ? -1.0 : 1.0;
		}
		tfar = std::min(tfar, t2);
		if (tnear > tfar)
			return tmax;
	}

	if (tfar < tmin || tnear >= tmax)
		return tmax;

	if (tnear < tmin)
	{
		// Starting inside the box: like UE1, don't trap the actor. Only stop it from going deeper
		// through the nearest face; any other movement (sliding along or leaving) is allowed.
		double best = 1e30;
		for (int i = 0; i < 3; i++)
		{
			double depth = half[i] - std::abs(localPos[i]);
			if (depth < best)
			{
				best = depth;
				nearAxis = i;
				nearSign = localPos[i] >= 0.0 ? 1.0 : -1.0;
			}
		}
		if (localDir[nearAxis] * nearSign >= 0.0)
			return tmax;
		tnear = tmin;
	}

	if (nearAxis < 0)
		return tmax;
	normal = box->Collision.BoxAxis[nearAxis] * (float)nearSign;
	return tnear;
}

void CollisionSystem::AddToCollision(UActor* actor)
{
	if (actor->bCollideActors())
	{
		vec3 location = actor->Location();
		float height = actor->CollisionHeight();
		float radius = actor->CollisionRadius();
		vec3 extents = { radius, radius, height };

		UpdateCollisionShape(actor);
		if (actor->Collision.IsBox)
		{
			// World bounds of the oriented box
			extents = vec3(0.0f);
			for (int i = 0; i < 3; i++)
			{
				vec3 a = actor->Collision.BoxAxis[i] * actor->Collision.BoxHalfSize[i];
				extents += vec3(std::abs(a.x), std::abs(a.y), std::abs(a.z));
			}
			extents += 0.1f;
		}

		if (UMover* mover = UObject::TryCast<UMover>(actor))
		{
			UModel* brush = mover->Brush();
			if (brush)
			{
				mat4 objectToWorld = mat4::translate(actor->Location()) * Coords::Rotation(actor->Rotation()).ToMatrix() * mat4::scale(mover->MainScale().Scale) * mat4::translate(-actor->PrePivot());
				BBox bbox = brush->BoundingBox.transform(objectToWorld);
				location = bbox.center();
				extents = bbox.extents() + 0.1f; // 0.1 for numerical stability
			}
		}

		actor->Collision.Inserted = true;
		actor->Collision.Location = location;
		actor->Collision.Extents = extents;

		ivec3 start = GetStartExtents(location, extents);
		ivec3 end = GetEndExtents(location, extents);
		for (int z = start.z; z < end.z; z++)
		{
			for (int y = start.y; y < end.y; y++)
			{
				for (int x = start.x; x < end.x; x++)
				{
					CollisionActors[GetBucketId(x, y, z)].push_back(actor);
				}
			}
		}
	}
}

void CollisionSystem::RemoveFromCollision(UActor* actor)
{
	if (actor->Collision.Inserted)
	{
		vec3 location = actor->Collision.Location;
		vec3 extents = actor->Collision.Extents;

		ivec3 start = GetStartExtents(location, extents);
		ivec3 end = GetEndExtents(location, extents);
		for (int z = start.z; z < end.z; z++)
		{
			for (int y = start.y; y < end.y; y++)
			{
				for (int x = start.x; x < end.x; x++)
				{
					auto it = CollisionActors.find(GetBucketId(x, y, z));
					if (it != CollisionActors.end())
					{
						it->second.remove(actor);
						if (it->second.empty())
							CollisionActors.erase(it);
					}
				}
			}
		}

		actor->Collision.Inserted = false;
	}
}
