#pragma once

#include <AL/al.h>
#include <AL/alc.h>
#include <AL/alext.h>
#include "Math/vec.h"

class UActor;
class USound;

// Where the sounds are heard from, in Unreal coordinates (X forward, Y right, Z up).
struct ALListenerFrame
{
	vec3 location = vec3(0.0f);
	vec3 forward = vec3(1.0f, 0.0f, 0.0f);
	vec3 right = vec3(0.0f, 1.0f, 0.0f);
};

class ALSoundSource
{
public:
	ALSoundSource();
	~ALSoundSource();

	void Play();
	void Stop();
	bool IsPlaying();
	USound* GetSound();
	void SetDopplerFactor(float newDopplerFactor);
	void SetPosition(const vec3& newPosition, const ALListenerFrame& listener);
	void UpdatePosition(const ALListenerFrame& listener);
	void SetRadius(float newRadius);
	void SetVelocity(vec3& newVelocity);
	void SetSound(USound* newSound);
	void SetSpatial(bool bSpatial);
	void SetVolume(float newVolume);
	void SetPitch(float newPitch);
	void DoLoop();

private:
	ALuint id = -1;
	UActor* actor = nullptr;
	USound* sound = nullptr;
	vec3 worldPosition = vec3(0.0f);
	vec3 velocity = vec3(0.0f);
	float radius = 0.0f;
	float volume = 0.0f;
	float pitch = 0.0f;
	float dopplerFactor = 0.0f;
	bool bIs3d = false;
};
