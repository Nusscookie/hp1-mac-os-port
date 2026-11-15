
#include "Precomp.h"
#include "ParticleFX.h"
#include "UActor.h"
#include "Packages/Engine/Actors/Info/UZoneInfo.h"
#include "Packages/Engine/Resources/Textures/UTexture.h"
#include "Packages/Engine/Resources/UPalette.h"
#include "Packages/Engine/Resources/Level/UModel.h"
#include "Render/VisibleFrame.h"
#include "Render/RenderSubsystem.h"
#include "RenderDevice/RenderDevice.h"
#include "Engine.h"

namespace
{
	// Script structs: FloatParams { float Base, Rand; }  ColorParams { color Base, Rand; }
	struct FloatParams { float Base; float Rand; };
	struct ColorParams { uint8_t Base[4]; uint8_t Rand[4]; };

	enum EParticlePrimitive { PPRIM_Line, PPRIM_Billboard, PPRIM_Liquid, PPRIM_Shard, PPRIM_TriTube };

	struct Offsets
	{
		bool Valid = false;
		PropertyDataOffset ParticlesPerSec, SourceWidth, SourceHeight, SourceDepth, AngularSpreadWidth, AngularSpreadHeight;
		PropertyDataOffset bSteadyState, bPrime, Speed, Lifetime, ColorStart, ColorEnd, AlphaStart, AlphaEnd;
		PropertyDataOffset SizeWidth, SizeLength, SizeEndScale, SpinRate;
		PropertyDataOffset bUpdate, bVelocityRelative, bSystemRelative, ParentBlend;
		PropertyDataOffset AlphaDelay, ColorDelay, SizeDelay, AlphaGrowPeriod, SizeGrowPeriod;
		PropertyDataOffset Chaos, ChaosDelay, Attraction, Damping, GravityModifier, Gravity;
		PropertyDataOffset ParticlesAlive, ParticlesMax, Textures, RenderPrimitive;
		PropertyDataOffset EmissionResidue, Age, ParticlesEmitted, bEmit;

		void Init(UActor* actor)
		{
			#define OFFSET(name) name = actor->GetPropertyDataOffset(#name)
			OFFSET(ParticlesPerSec); OFFSET(SourceWidth); OFFSET(SourceHeight); OFFSET(SourceDepth);
			OFFSET(AngularSpreadWidth); OFFSET(AngularSpreadHeight); OFFSET(bSteadyState); OFFSET(bPrime);
			OFFSET(Speed); OFFSET(Lifetime); OFFSET(ColorStart); OFFSET(ColorEnd); OFFSET(AlphaStart); OFFSET(AlphaEnd);
			OFFSET(SizeWidth); OFFSET(SizeLength); OFFSET(SizeEndScale); OFFSET(SpinRate);
			OFFSET(bUpdate); OFFSET(bVelocityRelative); OFFSET(bSystemRelative); OFFSET(ParentBlend);
			OFFSET(AlphaDelay); OFFSET(ColorDelay); OFFSET(SizeDelay); OFFSET(AlphaGrowPeriod); OFFSET(SizeGrowPeriod);
			OFFSET(Chaos); OFFSET(ChaosDelay); OFFSET(Attraction); OFFSET(Damping); OFFSET(GravityModifier); OFFSET(Gravity);
			OFFSET(ParticlesAlive); OFFSET(ParticlesMax); OFFSET(Textures); OFFSET(RenderPrimitive);
			OFFSET(EmissionResidue); OFFSET(Age); OFFSET(ParticlesEmitted); OFFSET(bEmit);
			#undef OFFSET
			Valid = true;
		}
	};

	Offsets offsets;

	template<typename T> T& Prop(UActor* actor, const PropertyDataOffset& offset)
	{
		return *static_cast<T*>(actor->PropertyData.Ptr(offset.DataOffset));
	}

	bool BoolProp(UActor* actor, const PropertyDataOffset& offset)
	{
		return (*static_cast<uint32_t*>(actor->PropertyData.Ptr(offset.DataOffset)) & offset.BitfieldMask) != 0;
	}

	float FRand() { return (float)std::rand() * (1.0f / (float)RAND_MAX); }
	float Pick(const FloatParams& p) { return p.Base + p.Rand * FRand(); }

	vec3 PickColor(const ColorParams& p)
	{
		vec3 c;
		c.x = clamp(((float)p.Base[0] + (float)p.Rand[0] * FRand()) * (1.0f / 255.0f), 0.0f, 1.0f);
		c.y = clamp(((float)p.Base[1] + (float)p.Rand[1] * FRand()) * (1.0f / 255.0f), 0.0f, 1.0f);
		c.z = clamp(((float)p.Base[2] + (float)p.Rand[2] * FRand()) * (1.0f / 255.0f), 0.0f, 1.0f);
		return c;
	}

	ParticleSystemState* GetState(UActor* actor)
	{
		if (!actor->ParticleState)
			actor->ParticleState = std::make_shared<ParticleSystemState>();
		return actor->ParticleState.get();
	}

	// Fraction (0 to 1) of the way through a transition that starts after `delay` seconds and ends with the particle's life
	float Transition(float age, float lifetime, float delay)
	{
		if (age <= delay || lifetime <= delay)
			return 0.0f;
		return clamp((age - delay) / (lifetime - delay), 0.0f, 1.0f);
	}

	void Emit(UActor* actor, ParticleSystemState* state, const vec3& location, float preAge)
	{
		Particle p;

		mat3 rotation = mat3(Coords::Rotation(actor->Rotation()).ToMatrix());

		// Somewhere inside the source box
		vec3 offset;
		offset.x = (FRand() - 0.5f) * Pick(Prop<FloatParams>(actor, offsets.SourceDepth));
		offset.y = (FRand() - 0.5f) * Pick(Prop<FloatParams>(actor, offsets.SourceWidth));
		offset.z = (FRand() - 0.5f) * Pick(Prop<FloatParams>(actor, offsets.SourceHeight));
		p.Position = rotation * offset;
		if (!BoolProp(actor, offsets.bSystemRelative))
			p.Position += location;

		// Forward, spread out by the angular spread (degrees)
		float yaw = radians((FRand() - 0.5f) * Pick(Prop<FloatParams>(actor, offsets.AngularSpreadWidth)));
		float pitch = radians((FRand() - 0.5f) * Pick(Prop<FloatParams>(actor, offsets.AngularSpreadHeight)));
		vec3 dir(std::cos(pitch) * std::cos(yaw), std::cos(pitch) * std::sin(yaw), std::sin(pitch));
		p.Velocity = (rotation * dir) * Pick(Prop<FloatParams>(actor, offsets.Speed));
		if (BoolProp(actor, offsets.bVelocityRelative))
			p.Velocity += actor->Velocity();

		p.Lifetime = std::max(Pick(Prop<FloatParams>(actor, offsets.Lifetime)), 0.01f);
		p.ColorStart = PickColor(Prop<ColorParams>(actor, offsets.ColorStart));
		p.ColorEnd = PickColor(Prop<ColorParams>(actor, offsets.ColorEnd));
		p.AlphaStart = Pick(Prop<FloatParams>(actor, offsets.AlphaStart));
		p.AlphaEnd = Pick(Prop<FloatParams>(actor, offsets.AlphaEnd));
		p.Width = Pick(Prop<FloatParams>(actor, offsets.SizeWidth));
		p.Length = Pick(Prop<FloatParams>(actor, offsets.SizeLength));
		p.EndScale = Pick(Prop<FloatParams>(actor, offsets.SizeEndScale));
		p.SpinRate = Pick(Prop<FloatParams>(actor, offsets.SpinRate));
		p.Spin = FRand() * 6.2831853f;
		p.Age = preAge;

		// One of the system's textures at random
		UTexture** textures = &Prop<UTexture*>(actor, offsets.Textures);
		int numTextures = 0;
		for (int i = 0; i < 5; i++)
			if (textures[i])
				numTextures++;
		if (numTextures > 0)
		{
			int pick = std::min((int)(FRand() * numTextures), numTextures - 1);
			for (int i = 0; i < 5; i++)
			{
				if (textures[i] && pick-- == 0)
				{
					p.Texture = textures[i];
					break;
				}
			}
		}
		else
		{
			p.Texture = actor->Texture();
		}

		state->Particles.push_back(p);
		Prop<int>(actor, offsets.ParticlesEmitted)++;
	}

	void Simulate(UActor* actor, ParticleSystemState* state, float elapsed)
	{
		vec3 gravity = Prop<vec3>(actor, offsets.Gravity);
		float gravityModifier = Prop<float>(actor, offsets.GravityModifier);
		if (gravityModifier != 0.0f && actor->Region().Zone)
			gravity += actor->Region().Zone->ZoneGravity() * gravityModifier;

		vec3 attraction = Prop<vec3>(actor, offsets.Attraction);
		float damping = Prop<float>(actor, offsets.Damping);
		float chaos = Prop<float>(actor, offsets.Chaos);
		float chaosDelay = Prop<float>(actor, offsets.ChaosDelay);
		vec3 origin = BoolProp(actor, offsets.bSystemRelative) ? vec3(0.0f) : actor->Location();

		size_t alive = 0;
		for (size_t i = 0; i < state->Particles.size(); i++)
		{
			Particle& p = state->Particles[i];
			p.Age += elapsed;
			if (p.Age >= p.Lifetime)
				continue;

			p.Velocity += gravity * elapsed;
			if (attraction.x != 0.0f || attraction.y != 0.0f || attraction.z != 0.0f)
				p.Velocity -= (p.Position - origin) * attraction * elapsed;
			if (damping != 0.0f)
				p.Velocity *= std::max(1.0f - damping * elapsed, 0.0f);
			if (chaos != 0.0f)
			{
				p.ChaosTimer += elapsed;
				if (p.ChaosTimer >= chaosDelay)
				{
					p.ChaosTimer = 0.0f;
					p.Velocity += vec3(FRand() - 0.5f, FRand() - 0.5f, FRand() - 0.5f) * (2.0f * chaos);
				}
			}

			p.Position += p.Velocity * elapsed;
			p.Spin += p.SpinRate * elapsed;

			if (alive != i)
				state->Particles[alive] = p;
			alive++;
		}
		state->Particles.resize(alive);
	}
}

bool ParticleFX::IsParticleSystem(UActor* actor)
{
	return actor->IsA("ParticleFX");
}

void ParticleFX::Tick(UActor* actor, float elapsed)
{
	if (!offsets.Valid)
		offsets.Init(actor);

	ParticleSystemState* state = GetState(actor);
	if (!state->Initialized)
	{
		state->Initialized = true;
		state->LastLocation = actor->Location();
	}

	Prop<float>(actor, offsets.Age) += elapsed;

	if (BoolProp(actor, offsets.bUpdate))
		Simulate(actor, state, elapsed);

	// Emission
	int particlesMax = Prop<int>(actor, offsets.ParticlesMax);
	int& emitted = Prop<int>(actor, offsets.ParticlesEmitted);
	float parentBlend = clamp(Prop<float>(actor, offsets.ParentBlend), 0.0f, 1.0f);
	bool canEmit = BoolProp(actor, offsets.bEmit) && (particlesMax <= 0 || emitted < particlesMax);
	if (canEmit)
	{
		// ParentBlend fades a system towards its (invisible) parent settings. Scripts use 1 to switch a system off.
		float rate = std::max(Pick(Prop<FloatParams>(actor, offsets.ParticlesPerSec)), 0.0f) * (1.0f - parentBlend);
		float& residue = Prop<float>(actor, offsets.EmissionResidue);
		residue += rate * elapsed;
		int count = (int)residue;
		residue -= (float)count;
		count = std::min(count, 500);
		if (particlesMax > 0)
			count = std::min(count, particlesMax - emitted);

		for (int i = 0; i < count; i++)
		{
			// Spread the new particles along the path the system moved this frame, and over the frame's time
			float t = (count > 1) ? (float)i / (float)(count - 1) : 1.0f;
			Emit(actor, state, mix(state->LastLocation, actor->Location(), t), elapsed * (1.0f - t));
		}
	}
	state->LastLocation = actor->Location();

	// Keep at most ParticlesAlive particles; the oldest go first
	int particlesAlive = Prop<int>(actor, offsets.ParticlesAlive);
	if (particlesAlive > 0 && state->Particles.size() > (size_t)particlesAlive)
		state->Particles.erase(state->Particles.begin(), state->Particles.begin() + (state->Particles.size() - (size_t)particlesAlive));

	// A system with a finite particle budget is done when all of them are gone (see ParticleFX.Shutdown)
	if (particlesMax > 0 && emitted >= particlesMax && state->Particles.empty())
		actor->Destroy();
}

void ParticleFX::Draw(VisibleFrame* frame, UActor* actor)
{
	if (!offsets.Valid || !actor->ParticleState)
		return;

	ParticleSystemState* state = actor->ParticleState.get();
	if (state->Particles.empty())
		return;

	float alphaDelay = Prop<float>(actor, offsets.AlphaDelay);
	float colorDelay = Prop<float>(actor, offsets.ColorDelay);
	float sizeDelay = Prop<float>(actor, offsets.SizeDelay);
	float alphaGrow = Prop<float>(actor, offsets.AlphaGrowPeriod);
	float sizeGrow = Prop<float>(actor, offsets.SizeGrowPeriod);
	float visibility = 1.0f - clamp(Prop<float>(actor, offsets.ParentBlend), 0.0f, 1.0f);
	int primitive = Prop<uint8_t>(actor, offsets.RenderPrimitive);
	vec3 systemOrigin = BoolProp(actor, offsets.bSystemRelative) ? actor->Location() : vec3(0.0f);

	uint32_t renderflags = PF_TwoSided | PF_Unlit;
	switch (actor->Style())
	{
	case STY_Masked: renderflags |= PF_Masked; break;
	case STY_Modulated: renderflags |= PF_Modulated; break;
	case STY_Translucent:
	default: renderflags |= PF_Translucent; break;
	}

	vec3 viewLocation = frame->ViewLocation.xyz();
	UTexture* boundTexture = nullptr;
	TextureInfo texinfo;
	float texwidth = 1.0f, texheight = 1.0f;

	for (const Particle& p : state->Particles)
	{
		UTexture* texture = p.Texture ? p.Texture->GetAnimTexture() : nullptr;
		if (!texture)
			continue;

		if (texture != boundTexture)
		{
			engine->render->UpdateTexture(texture);
			texinfo = {};
			engine->render->UpdateTextureInfo(texinfo, texture);
			texwidth = (float)texture->UsedMipmaps.front().Width;
			texheight = (float)texture->UsedMipmaps.front().Height;
			boundTexture = texture;
		}

		float life = p.Age / p.Lifetime;

		float alpha = mix(p.AlphaStart, p.AlphaEnd, Transition(p.Age, p.Lifetime, alphaDelay));
		if (alphaGrow > 0.0f && life < alphaGrow)
			alpha *= life / alphaGrow;
		alpha = clamp(alpha, 0.0f, 1.0f) * visibility;
		if (alpha <= 0.002f)
			continue;

		vec3 color = mix(p.ColorStart, p.ColorEnd, Transition(p.Age, p.Lifetime, colorDelay)) * alpha;

		float scale = mix(1.0f, p.EndScale, Transition(p.Age, p.Lifetime, sizeDelay));
		if (sizeGrow > 0.0f && life < sizeGrow)
			scale *= life / sizeGrow;
		float halfWidth = p.Width * scale * 0.5f;
		float halfLength = p.Length * scale * 0.5f;
		if (halfWidth <= 0.0f || halfLength <= 0.0f)
			continue;

		vec3 center = p.Position + systemOrigin;
		vec3 sideAxis, upAxis;

		float speed = length(p.Velocity);
		if (primitive == PPRIM_Line && speed > 0.001f)
		{
			// A streak along the direction of travel, turned to face the camera
			upAxis = p.Velocity * (halfLength / speed);
			vec3 toCamera = viewLocation - center;
			vec3 side = cross(p.Velocity, toCamera);
			float sideLength = length(side);
			sideAxis = (sideLength > 0.0001f) ? side * (halfWidth / sideLength) : frame->ViewRotation.YAxis * halfWidth;
		}
		else
		{
			// Camera facing quad, rotated by the particle's spin
			float c = std::cos(p.Spin), s = std::sin(p.Spin);
			sideAxis = (frame->ViewRotation.YAxis * c + frame->ViewRotation.ZAxis * s) * halfWidth;
			upAxis = (frame->ViewRotation.ZAxis * c - frame->ViewRotation.YAxis * s) * halfLength;
		}

		GouraudVertex vertices[4];
		vertices[0].Point = center - sideAxis - upAxis;
		vertices[0].UV = { 0.0f, 0.0f };
		vertices[1].Point = center + sideAxis - upAxis;
		vertices[1].UV = { texwidth, 0.0f };
		vertices[2].Point = center + sideAxis + upAxis;
		vertices[2].UV = { texwidth, texheight };
		vertices[3].Point = center - sideAxis + upAxis;
		vertices[3].UV = { 0.0f, texheight };
		for (GouraudVertex& v : vertices)
		{
			v.Light = color;
			v.Fog = vec4(0.0f);
		}

		frame->Device->DrawGouraudPolygon(&frame->Frame, texinfo, vertices, 4, renderflags | texture->PolyFlags());
	}

	engine->render->Stats.Actors++;
}
