
#include "Precomp.h"
#include "UActor.h"
#include "ParticleFX.h"
#include "Packages/Core/UClass.h"
#include "Packages/Engine/UViewport.h"
#include "Packages/Engine/Actors/USpawnNotify.h"
#include "Packages/Engine/Actors/Info/ULevelInfo.h"
#include "Packages/Engine/Actors/Info/UGameInfo.h"
#include "Packages/Engine/Actors/Pawn/UPlayerPawn.h"
#include "Packages/Engine/Resources/Level/ULevel.h"
#include "Packages/Engine/Resources/Level/UModel.h"
#include "Packages/Engine/Subsystems/USurrealAudioDevice.h"
#include "Utils/Logger.h"
#include <algorithm>
#include "Packages/Engine/Resources/Textures/UTexture.h"
#include "Package/PackageManager.h"
#include "Utils/File.h"
#include "Engine.h"
#include "VM/ScriptCall.h"
#include "VM/Frame.h"

UActor* UActor::Spawn(UClass* SpawnClass, std::optional<UActor*> SpawnOwner, std::optional<NameString> SpawnTag, std::optional<vec3> SpawnLocation, std::optional<Rotator> SpawnRotation)
{
	if (!SpawnClass || SpawnClass->ClsFlags & ClassFlags::Abstract)
	{
		LogMessage("Could not spawn class: " + (SpawnClass ? SpawnClass->Name.ToString() : std::string("null")));
		return nullptr;
	}

	vec3 location = SpawnLocation ? *SpawnLocation : Location();
	Rotator rotation = SpawnRotation ? *SpawnRotation : Rotation();

	float radius = SpawnClass->GetDefaultObject<UActor>()->CollisionRadius();
	float height = SpawnClass->GetDefaultObject<UActor>()->CollisionHeight();
	bool bCollideWorld = SpawnClass->GetDefaultObject<UActor>()->bCollideWorld();
	bool bCollideWhenPlacing = SpawnClass->GetDefaultObject<UActor>()->bCollideWhenPlacing();
	if (bCollideWorld || bCollideWhenPlacing)
	{
		auto result = CheckLocation(location, radius, height, bCollideWorld || bCollideWhenPlacing);
		if (!result.first)
		{
			LogMessage("Could not find usable location when trying to spawn: " + SpawnClass->Name.ToString());
			return nullptr;
		}
		location = result.second;
	}

	// To do: package needs to be grabbed from outer, or the "transient package" if it is None, a virtual package for runtime objects
	// To do: find unique new name in the package
	static std::map<NameString, int> nextIndex;
	NameString name = SpawnClass->Name.ToString() + std::to_string(nextIndex[SpawnClass->Name]++);
	UActor* actor = UObject::Cast<UActor>(engine->LevelPackage->NewObject(name, UObject::Cast<UClass>(SpawnClass), ObjectFlags::Transient, true));

	actor->Outer() = XLevel()->Outer();
	actor->XLevel() = XLevel();
	actor->Level() = Level();
	actor->Tag() = (SpawnTag && !SpawnTag->IsNone()) ? *SpawnTag : SpawnClass->Name;
	actor->bTicked() = bTicked(); // To do: should it tick in the same world tick it was spawned in or wait until the next one?
	actor->Instigator() = Instigator();
	actor->Brush() = nullptr;
	actor->Location() = location;
	actor->OldLocation() = location;
	actor->Rotation() = rotation;
	actor->Region().Zone = actor->Level();
	actor->Index = (int)XLevel()->Actors.size();
	XLevel()->Actors.push_back(actor);
	XLevel()->Collision.AddToCollision(actor);

	actor->SetOwner(SpawnOwner.has_value() && SpawnOwner.value() ? *SpawnOwner : nullptr);

	if (Level()->bBegunPlay())
	{
		CallEvent(actor, EventName::Spawned);
		CallEvent(actor, EventName::PreBeginPlay);
		CallEvent(actor, EventName::BeginPlay);

		if (actor->bDeleteMe())
		{
			//LogMessage("Object destroyed itself during Spawn!");
			return nullptr;
		}

		// To do: we need to call EventName::EncroachingOn events here?

		actor->InitActorZone();

		CallEvent(actor, EventName::PostBeginPlay);
		CallEvent(actor, EventName::SetInitialState);
		if (engine->LaunchInfo.IsDeusEx())
			CallEvent(actor, "PostPostBeginPlay");

		actor->InitBase();

		if (engine->LaunchInfo.ue1Version >= 400)
		{
			static bool spawnNotificationLocked = false;
			if (!spawnNotificationLocked)
			{
				struct NotificationLockGuard
				{
					NotificationLockGuard() { spawnNotificationLocked = true; }
					~NotificationLockGuard() { spawnNotificationLocked = false; }
				} lockGuard;

				for (USpawnNotify* notifyObj = Level()->SpawnNotify(); notifyObj != nullptr; notifyObj = notifyObj->Next())
				{
					UClass* cls = notifyObj->ActorClass();
					if (cls && actor->IsA(cls->Name))
						actor = UObject::Cast<UGameInfo>(CallEvent(notifyObj, EventName::SpawnNotification, { ExpressionValue::ObjectValue(actor) }).ToObject());
				}
			}
		}
	}

	return actor;
}

bool UActor::Destroy()
{
	//engine->LogMessage("UActor.Destroy(" + Class->FriendlyName.ToString() + ")");

	if (bStatic() || bNoDelete())
		return false;
	if (bDeleteMe())
		return true;

	bDeleteMe() = true;

	//GotoState({}, {}); // What should happen to function calls after Destroy() has been called? Razor2 calls SetRoll afterwards!
	SetBase(nullptr, true);

	engine->audiodev->ActorDestroyed(this);

	ULevel* level = XLevel();

	RemoveFromBspNode();
	level->Collision.RemoveFromCollision(this);

	CallEvent(this, EventName::Destroyed);

	if (engine->LaunchInfo.IsUnrealTournament_469())
	{
		for (const auto actor : Touching_UT469())
			if (actor)
				UnTouch(actor);
	}
	else
	{
		for (const auto actor : Touching())
			if (actor)
				UnTouch(actor);
	}


	SetOwner(nullptr);

	while (!ChildActors.empty())
	{
		ChildActors.back()->SetOwner(nullptr);
	}
	while (!BasedActors.empty())
	{
		BasedActors.back()->SetBase(nullptr, true);
	}

	if (Index == -1)
		throw std::runtime_error("Actor index was never set!");
	level->Actors[Index] = nullptr;

	return true;
}

void UActor::Tick(float elapsed)
{
	if (engine->LaunchInfo.IsDeusEx())
	{
		DistanceFromPlayer() = length(engine->viewport->Actor()->Location() - Location());
	}

	TickAnimation(elapsed);
	if (engine->LaunchInfo.IsDeusEx())
		TickBlendAnimation(elapsed);

	if (engine->LaunchInfo.IsHarryPotter1() && (ParticleState || (DrawType() == DT_Particles && ParticleFX::IsParticleSystem(this))))
	{
		ParticleFX::Tick(this, elapsed);
		if (bDeleteMe())
			return;
	}

	if (Role() >= ROLE_SimulatedProxy && IsEventEnabled(EventName::Tick))
	{
		CallEvent(this, EventName::Tick, { ExpressionValue::FloatValue(elapsed) });
	}

	if (StateFrame)
	{
		LatentRunState latentState = StateFrame->GetLatentState();
		if (latentState == LatentRunState::Sleep)
		{
			SleepTimeLeft = std::max(SleepTimeLeft - elapsed, 0.0f);
			if (SleepTimeLeft == 0.0f)
				StateFrame->SetLatentState(LatentRunState::Continue);
		}
		else if (latentState == LatentRunState::FinishInterpolation)
		{
			if (!bInterpolating())
				StateFrame->SetLatentState(LatentRunState::Continue);
		}

		if (Role() >= ROLE_SimulatedProxy && StateFrame->GetLatentState() == LatentRunState::Continue)
		{
			StateFrame->Tick();
		}
	}

	TickPhysics(elapsed);

	if (TimerRate() > 0.0f) // Role() == ROLE_Authority && RemoteRole() == ROLE_AutonomousProxy
	{
		TimerCounter() += elapsed;
		while (TimerRate() > 0.0f && TimerCounter() > TimerRate())
		{
			TimerCounter() -= TimerRate();
			if (!bTimerLoop())
				TimerRate() = 0.0f;
			CallEvent(this, EventName::Timer);
		}
	}
}

bool UActor::Move(const vec3& delta)
{
	return TryMove(delta).Fraction == 1.0f;
}

bool UActor::MoveSmooth(const vec3& delta)
{
	CollisionHit hit = TryMoveSmooth(delta);
	return hit.Fraction != 1.0f;
}

void UActor::MakeNoise(float loudness)
{
	UPawn* noisePawn = UObject::Cast<UPawn>(Instigator());

	if (!noisePawn || Level()->NetMode() == NM_Client)
		return;

	float currentTime = Level()->TimeSeconds();
	vec3 delta1 = noisePawn->noise1spot() - Location();
	vec3 delta2 = noisePawn->noise2spot() - Location();
	if ((noisePawn->noise1time() > currentTime - 0.2f && dot(delta1, delta1) < 2500.0f && noisePawn->noise1loudness() >= 0.9f * loudness) ||
		(noisePawn->noise2time() > currentTime - 0.2f && dot(delta2, delta2) < 2500.0f && noisePawn->noise2loudness() >= 0.9f * loudness))
	{
		return;
	}

	if (noisePawn->noise1time() < currentTime - 0.18f)
	{
		noisePawn->noise1time() = currentTime;
		noisePawn->noise1spot() = Location();
		noisePawn->noise1loudness() = loudness;
	}
	else if (noisePawn->noise2time() < currentTime - 0.18f)
	{
		noisePawn->noise2time() = currentTime;
		noisePawn->noise2spot() = Location();
		noisePawn->noise2loudness() = loudness;
	}
	else if (dot(delta1, delta1) < 2500.0f)
	{
		noisePawn->noise1time() = currentTime;
		noisePawn->noise1spot() = Location();
		noisePawn->noise1loudness() = loudness;
	}
	else if (noisePawn->noise2loudness() <= loudness)
	{
		noisePawn->noise2time() = currentTime;
		noisePawn->noise2spot() = Location();
		noisePawn->noise2loudness() = loudness;
	}

	for (UPawn* pawn = Level()->PawnList(); pawn != nullptr; pawn = pawn->nextPawn())
	{
		if (pawn != noisePawn && pawn->CanHearNoise(this, loudness))
		{
			CallEvent(pawn, EventName::HearNoise, { ExpressionValue::FloatValue(loudness), ExpressionValue::ObjectValue(this) });
		}
	}
}

bool UActor::PlayerCanSeeMe()
{
	for (UPawn* pawn = Level()->PawnList(); pawn != nullptr; pawn = pawn->nextPawn())
	{
		if (pawn == this)
			continue;

		vec3 L = Location() - pawn->Location();
		float dist2 = dot(L, L);

		// Too far away
		if (dist2 > 500 * 500)
			continue;

		// Without behind view the pawn can only see in a 75 degree cone in front of them
		if (!pawn->bBehindView())
		{
			vec3 viewDirection = Coords::Rotation(pawn->ViewRotation()).XAxis;
			if (dot(viewDirection, L) < 0.2588190451f * dist2)
				continue;
		}

		// Try check for line of sight
		vec3 eyePos = pawn->Location();
		eyePos.z += pawn->BaseEyeHeight();
		if (pawn->FastTrace(Location(), eyePos))
			return true;
	}
	return false;
}

// Harry Potter 1 save support.
// Script passes Windows paths relative to the System folder, e.g. "..\Save\SGS Lev_tut1.bmp".
static fs::path ResolveGamePath(const std::string& path)
{
	std::string p = path;
	std::replace(p.begin(), p.end(), '\\', '/');
	fs::path result(p);
	if (result.is_relative())
		result = engine->packages->GetSystemFolderPath() / result;
	return result.lexically_normal();
}

// GameSaveInfo-style objects: every script property declared below Core.Object, one "Name=Value" per line.
static fs::path ObjectFilePath(const std::string& dir)
{
	fs::path path = engine->packages->GetSaveFolderPath() / (dir + ".txt");
	return path.lexically_normal();
}

static Array<UProperty*> GetScriptProperties(UObject* object)
{
	Array<UProperty*> result;
	UClass* objectClass = engine->packages->FindClass("Core.Object");
	for (UProperty* prop : object->GetAllProperties())
	{
		if (AllFlags(prop->PropFlags, PropertyFlags::Native) || AllFlags(prop->PropFlags, PropertyFlags::Transient))
			continue;
		bool inObject = false;
		if (objectClass)
		{
			for (UProperty* baseProp : objectClass->Properties)
				inObject = inObject || baseProp == prop;
		}
		if (!inObject)
			result.push_back(prop);
	}
	return result;
}

static bool SaveObjectProperties(const fs::path& filename, UObject* object)
{
	if (!object)
		return false;
	try
	{
		fs::create_directories(filename.parent_path());
		std::string text;
		for (UProperty* prop : GetScriptProperties(object))
			text += prop->Name.ToString() + "=" + object->GetPropertyAsString(prop->Name) + "\n";
		File::write_all_text(filename.string(), text);
		return true;
	}
	catch (const std::exception& e)
	{
		LogMessage("Could not save " + filename.string() + ": " + e.what());
		return false;
	}
}

static bool LoadObjectProperties(const fs::path& filename, UObject* object)
{
	if (!object || !fs::exists(filename))
		return false;
	try
	{
		Array<UProperty*> props = GetScriptProperties(object);
		for (const std::string& line : File::read_all_lines(filename.string()))
		{
			size_t eq = line.find('=');
			if (eq == std::string::npos)
				continue;
			NameString name(line.substr(0, eq));
			for (UProperty* prop : props)
			{
				if (prop->Name == name)
				{
					std::string value = line.substr(eq + 1);
					if (!value.empty() && value.back() == '\r')
						value.pop_back();
					object->SetPropertyFromString(name, value);
					break;
				}
			}
		}
		return true;
	}
	catch (const std::exception& e)
	{
		LogMessage("Could not load " + filename.string() + ": " + e.what());
		return false;
	}
}

static UTexture* CreateRuntimeTexture(const std::string& name, int width, int height, Array<uint8_t> bgra)
{
	UClass* textureClass = engine->packages->FindClass("Engine.Texture");
	Package* transient = engine->packages->GetTransientPackage();
	auto texture = UObject::Cast<UTexture>(transient->NewObject(name, textureClass, ObjectFlags::Transient));

	int ubits = 0, vbits = 0;
	while ((1 << ubits) < width) ubits++;
	while ((1 << vbits) < height) vbits++;

	texture->Format() = (uint8_t)TextureFormat::BGRA8;
	texture->USize() = width;
	texture->VSize() = height;
	texture->UBits() = ubits;
	texture->VBits() = vbits;
	texture->UClamp() = width;
	texture->VClamp() = height;

	UnrealMipmap mip;
	mip.Width = width;
	mip.Height = height;
	mip.UBits = ubits;
	mip.VBits = vbits;
	mip.Data = std::move(bgra);
	texture->UncompressedMipmaps.push_back(mip);
	texture->UsedMipmaps = texture->UncompressedMipmaps;
	texture->UsedFormat = TextureFormat::BGRA8;
	return texture;
}

// Uncompressed 8-bit paletted, 24-bit and 32-bit Windows bitmaps (what the game ships in Save/).
static bool DecodeBMP(const Array<uint8_t>& file, int& width, int& height, Array<uint8_t>& bgra)
{
	auto get16 = [&](size_t o) { return (uint32_t)file[o] | ((uint32_t)file[o + 1] << 8); };
	auto get32 = [&](size_t o) { return get16(o) | (get16(o + 2) << 16); };
	if (file.size() < 54 || file[0] != 'B' || file[1] != 'M')
		return false;

	uint32_t dataOffset = get32(10);
	uint32_t headerSize = get32(14);
	int32_t w = (int32_t)get32(18);
	int32_t h = (int32_t)get32(22);
	int bpp = get16(28);
	uint32_t compression = get32(30);
	uint32_t paletteCount = get32(46);
	if (w <= 0 || h == 0 || compression != 0 || (bpp != 8 && bpp != 24 && bpp != 32))
		return false;

	bool topDown = h < 0;
	if (topDown) h = -h;
	if (bpp == 8 && paletteCount == 0) paletteCount = 256;
	size_t paletteOffset = 14 + headerSize;
	size_t pitch = (((size_t)w * bpp + 31) / 32) * 4;
	if (dataOffset + pitch * h > file.size() || (bpp == 8 && paletteOffset + paletteCount * 4 > file.size()))
		return false;

	width = w;
	height = h;
	bgra.resize((size_t)w * h * 4);
	for (int y = 0; y < h; y++)
	{
		const uint8_t* src = file.data() + dataOffset + pitch * (topDown ? y : h - 1 - y);
		uint8_t* dst = bgra.data() + (size_t)y * w * 4;
		for (int x = 0; x < w; x++, dst += 4)
		{
			if (bpp == 8)
			{
				uint32_t index = std::min<uint32_t>(src[x], paletteCount - 1);
				const uint8_t* c = file.data() + paletteOffset + index * 4;
				dst[0] = c[0]; dst[1] = c[1]; dst[2] = c[2];
			}
			else
			{
				const uint8_t* c = src + x * (bpp / 8);
				dst[0] = c[0]; dst[1] = c[1]; dst[2] = c[2];
			}
			dst[3] = 255;
		}
	}
	return true;
}

UTexture* UActor::CreateTextureFromScreenShot(UViewport* vport)
{
	// Called through "Snap 3" for save thumbnails; the shipped game reads its thumbnails from Save/SGS *.bmp instead.
	LogUnimplemented("Actor.CreateTextureFromScreenShot");
	return nullptr;
}

UTexture* UActor::CreateTextureFromBMP(const std::string& name, const std::string& filename)
{
	fs::path path = ResolveGamePath(filename);
	if (!fs::exists(path))
		return nullptr;

	int width = 0, height = 0;
	Array<uint8_t> pixels;
	if (!DecodeBMP(File::read_all_bytes(path.string()), width, height, pixels))
	{
		LogMessage("Unsupported bitmap: " + path.string());
		return nullptr;
	}
	return CreateRuntimeTexture(name, width, height, std::move(pixels));
}

bool UActor::SaveObjectAsFile(const std::string& dir, UObject* object)
{
	return SaveObjectProperties(ObjectFilePath(dir), object);
}

bool UActor::LoadObjectAsFile(const std::string& dir, UObject* object)
{
	return LoadObjectProperties(ObjectFilePath(dir), object);
}

bool UActor::SaveGameSaveInfo(const std::string& dir, UObject* object)
{
	return SaveObjectProperties(ObjectFilePath(dir), object);
}

bool UActor::LoadGameSaveInfo(const std::string& dir, UObject* object)
{
	return LoadObjectProperties(ObjectFilePath(dir), object);
}

bool UActor::IsOSVer2kOrXP()
{
	return true;
}

void UActor::AIClearEvent(const NameString& eventName)
{
	LogUnimplemented("Actor.AIClearEvent");
}

void UActor::AIClearEventCallback(const NameString& eventName)
{
	LogUnimplemented("Actor.AIClearEventCallback");
}

void UActor::AIEndEvent(const NameString& eventName, uint8_t eventType)
{
	LogUnimplemented("Actor.AIEndEvent");
}

float UActor::AIGetLightLevel(const vec3& Location)
{
	LogUnimplemented("Actor.AIGetLightLevel");
	return 1.0f;
}

void UActor::AISendEvent(const NameString& eventName, uint8_t eventType, std::optional<float> Value, std::optional<float> Radius)
{
	LogUnimplemented("Actor.AISendEvent");
	//LogUnimplemented(Name.ToString() + ": AISendEvent('" + eventName.ToString() + "')");
}

void UActor::AISetEventCallback(const NameString& eventName, const NameString& callback, std::optional<NameString> scoreCallback, std::optional<bool> bCheckVisibility, std::optional<bool> bCheckDir, std::optional<bool> bCheckCylinder, std::optional<bool> bCheckLOS)
{
	LogUnimplemented("Actor.AISetEventCallback");
	//LogMessage(Name.ToString() + ": AISetEventCallback('" + eventName.ToString() + "')");
}

void UActor::AIStartEvent(const NameString& eventName, uint8_t eventType, std::optional<float> Value, std::optional<float> Radius)
{
	LogUnimplemented("Actor.AIStartEvent");
	//LogUnimplemented(Name.ToString() + ": AIStartEvent('" + eventName.ToString() + "')");
}

float UActor::AIVisibility(std::optional<bool> bIncludeVelocity)
{
	LogUnimplemented("Actor.AIVisibility");
	//LogMessage(Name.ToString() + ": AIVisibility");
	return 0.0f;
}
