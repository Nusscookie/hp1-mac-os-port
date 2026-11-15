#pragma once

#include "Math/vec.h"
#include "Utils/Array.h"

class UActor;
class UTexture;
class VisibleFrame;

// Harry Potter 1 particle systems (Engine.ParticleFX, used for spells, sparkles, fire, smoke and trails).
// The scripts only configure these actors; emitting, moving and drawing the particles is done by the engine.

struct Particle
{
	vec3 Position = vec3(0.0f);
	vec3 Velocity = vec3(0.0f);
	float Age = 0.0f;
	float Lifetime = 1.0f;
	vec3 ColorStart = vec3(1.0f);
	vec3 ColorEnd = vec3(1.0f);
	float AlphaStart = 1.0f;
	float AlphaEnd = 0.0f;
	float Width = 1.0f;
	float Length = 1.0f;
	float EndScale = 1.0f;
	float Spin = 0.0f;
	float SpinRate = 0.0f;
	float ChaosTimer = 0.0f;
	UTexture* Texture = nullptr;
};

struct ParticleSystemState
{
	Array<Particle> Particles;
	bool Initialized = false;
	vec3 LastLocation = vec3(0.0f);
};

class ParticleFX
{
public:
	static bool IsParticleSystem(UActor* actor);
	static void Tick(UActor* actor, float elapsed);
	static void Draw(VisibleFrame* frame, UActor* actor);
};
