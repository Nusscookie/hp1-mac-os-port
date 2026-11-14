#pragma once

#include "Packages/Core/UObject.h"
#include "Math/quaternion.h"
#include "UMesh.h"

struct RefBone
{
	NameString Name;
	uint32_t Flags = 0;
	uint32_t ParentIndex = 0;
};

struct AnimTrack
{
	uint32_t Flags = 0;
	Array<quaternion> KeyQuat;
	Array<vec3> KeyPos;
	Array<float> KeyTime;
};

struct AnimMove
{
	vec3 RootSpeed3D;
	float TrackTime = 0.0f;
	uint32_t StartBone = 0;
	uint32_t Flags = 0;
	Array<uint32_t> BoneIndices;
	Array<AnimTrack> AnimTracks;
	AnimTrack RootTrack;
};

class UAnimation : public UObject
{
public:
	using UObject::UObject;

	void Load(ObjectStream* stream) override;
	void Save(PackageStreamWriter* stream) override;

	// Index of the move that holds the given sequence, or -1.
	// Harry Potter 1 stores one move per sequence and keeps the sequence list in the animation instead of the mesh.
	int FindMove(const NameString& sequence) const
	{
		for (size_t i = 0; i < AnimSeqs.size() && i < Moves.size(); i++)
			if (AnimSeqs[i].Name == sequence)
				return (int)i;
		return -1;
	}

	int FindBone(const NameString& name) const
	{
		for (size_t i = 0; i < RefBones.size(); i++)
			if (RefBones[i].Name == name)
				return (int)i;
		return -1;
	}

	Array<RefBone> RefBones;
	Array<AnimMove> Moves;
	Array<MeshAnimSeq> AnimSeqs;

private:
	void LoadHarryPotter1(ObjectStream* stream);
};
