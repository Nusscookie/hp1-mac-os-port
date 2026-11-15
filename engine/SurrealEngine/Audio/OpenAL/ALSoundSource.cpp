
#include "Precomp.h"
#include "ALSoundSource.h"
#include <algorithm>
#include <cmath>
#include "Packages/Engine/Actors/UActor.h"
#include "Packages/Engine/Resources/USound.h"

ALSoundSource::ALSoundSource()
{
	alGenSources(1, &id);
	if (alGetError() != AL_NO_ERROR)
		Exception::Throw("Failed to generate AL source");

	alSourcef(id, AL_ROLLOFF_FACTOR, 1.1f);
	// Positions are given relative to the listener, see UpdatePosition.
	alSourcei(id, AL_SOURCE_RELATIVE, AL_TRUE);
}

ALSoundSource::~ALSoundSource()
{
	Stop();
	alSourcei(id, AL_BUFFER, 0);
	alDeleteSources(1, &id);
}

void ALSoundSource::Play()
{
	alSourcePlay(id);
	if (alGetError() != AL_NO_ERROR)
		Exception::Throw("Failed to play AL source");
}

void ALSoundSource::Stop()
{
	alSourceStop(id);
}

bool ALSoundSource::IsPlaying()
{
	if (!alIsSource(id))
		return false;

	ALint state;
	alGetSourcei(id, AL_SOURCE_STATE, &state);
	return state == AL_PLAYING;
}

USound* ALSoundSource::GetSound()
{
	return sound;
}

void ALSoundSource::SetDopplerFactor(float newDopplerFactor)
{
	if (dopplerFactor != newDopplerFactor)
	{
		dopplerFactor = newDopplerFactor;
		alSourcef(id, AL_DOPPLER_FACTOR, dopplerFactor);
	}
}

void ALSoundSource::SetPosition(const vec3& newPosition, const ALListenerFrame& listener)
{
	worldPosition = newPosition;
	UpdatePosition(listener);
}

void ALSoundSource::UpdatePosition(const ALListenerFrame& listener)
{
	// OpenAL's stereo panning puts anything 60 degrees or more off-center entirely into one ear.
	// UE1 panned far more gently, so keep the distance (for attenuation) but squeeze the angle:
	// a sound fully to the side ends up at MaxPanAngle, which plays roughly 75/25 on both channels.
	const float MaxPanAngle = 25.0f * 3.14159265f / 180.0f;
	vec3 delta = worldPosition - listener.location;
	float distance = length(delta);
	vec3 relative(0.0f);
	if (distance > 0.001f)
	{
		float side = std::clamp(dot(delta, listener.right) / distance, -1.0f, 1.0f);
		float angle = std::asin(side) * (MaxPanAngle / (3.14159265f * 0.5f));
		relative = vec3(std::sin(angle) * distance, 0.0f, -std::cos(angle) * distance); // OpenAL: +x right, -z ahead
	}
	alSourcefv(id, AL_POSITION, &relative[0]);
}

void ALSoundSource::SetRadius(float newRadius)
{
	if (radius != newRadius)
	{
		radius = newRadius;
		alSourcef(id, AL_MAX_DISTANCE, radius);
		alSourcef(id, AL_REFERENCE_DISTANCE, 0.1f * radius);
	}
}

void ALSoundSource::SetVelocity(vec3& newVelocity)
{
	velocity.x = newVelocity.x;
	velocity.y = newVelocity.y;
	velocity.z = -newVelocity.z;
	alSourcefv(id, AL_VELOCITY, &velocity[0]);
}

void ALSoundSource::SetSound(USound* newSound)
{
	if (sound != newSound)
	{
		sound = newSound;
		alSourcei(id, AL_BUFFER, 0);
		alGetError();
		alSourcei(id, AL_BUFFER, (ALint)(ptrdiff_t)sound->handle);

		if (sound->loopInfo.Looped)
			alSourcei(id, AL_LOOPING, AL_TRUE);
		else
			alSourcei(id, AL_LOOPING, AL_FALSE);
	}
}

void ALSoundSource::SetSpatial(bool bSpatial)
{
	if (bIs3d != bSpatial)
	{
		bIs3d = bSpatial;
		alSourcei(id, AL_SOURCE_SPATIALIZE_SOFT, bIs3d);
	}
}

void ALSoundSource::SetVolume(float newVolume)
{
	if (volume != newVolume)
	{
		volume = newVolume;
		alSourcef(id, AL_GAIN, volume);
		alSourcef(id, AL_MAX_GAIN, volume);
	}
}

void ALSoundSource::SetPitch(float newPitch)
{
	if (pitch != newPitch)
	{
		pitch = newPitch;
		alSourcef(id, AL_PITCH, pitch);
	}
}

void ALSoundSource::DoLoop()
{
	if (sound->loopInfo.Looped)
	{
		ALint offset;
		alGetSourcei(id, AL_SAMPLE_OFFSET, &offset);
		if (offset >= sound->loopInfo.LoopEnd)
			alSourcei(id, AL_SAMPLE_OFFSET, (ALint)sound->loopInfo.LoopStart);
	}
}
