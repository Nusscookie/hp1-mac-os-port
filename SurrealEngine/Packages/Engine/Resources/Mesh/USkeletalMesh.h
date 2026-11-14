#pragma once

#include "ULodMesh.h"

struct ExtMeshWedge
{
	uint16_t Vertex;
	uint16_t Flags;
	float U;
	float V;
};

struct RefSkeletonBone
{
	NameString Name;
	uint32_t Flags;
	quaternion Orientation;
	vec3 Position;
	float Length;
	vec3 Size;
	uint32_t NumChildren;
	uint32_t ParentIndex;
};

struct BoneWeightIndex
{
	uint16_t WeightIndex;
	uint16_t Number;
	uint16_t DetailA;
	uint16_t DetailB;
};

struct BoneWeight
{
	uint16_t PointIndex;
	uint16_t BoneWeight;
};

// Bone transforms relative to the parent bone, one entry per bone of the mesh's reference skeleton
struct SkeletalPose
{
	Array<quaternion> Rotations;
	Array<vec3> Positions;

	bool IsValidFor(size_t numBones) const { return Rotations.size() == numBones && Positions.size() == numBones; }
};

class USkeletalMesh : public ULodMesh
{
public:
	using ULodMesh::ULodMesh;

	void Load(ObjectStream* stream) override;
	void Save(PackageStreamWriter* stream) override;

	int FindBone(const NameString& name) const;

	// Reference (bind) pose
	void GetRefPose(SkeletalPose& pose) const;

	// Pose of one animation move at the given time in seconds. Bones without a track keep the reference pose.
	void GetAnimPose(UAnimation* anim, int moveIndex, float time, SkeletalPose& pose);

	// Moves a pose towards another. alpha 0 keeps the pose, 1 replaces it. Only bones with a mask entry (if given) are affected.
	static void BlendPose(SkeletalPose& pose, const SkeletalPose& target, float alpha, const Array<uint8_t>* boneMask = nullptr);

	// Bone to mesh space matrices for a pose
	void GetBoneMatrices(const SkeletalPose& pose, Array<mat4>& bones) const;

	// Skinned vertex positions (mesh space) and smoothed normals
	void SkinVertices(const Array<mat4>& bones, Array<vec3>& verts, Array<vec3>& normals) const;

	// 1 for the given bone and all its descendants, 0 for the rest
	void GetBoneMask(int rootBone, Array<uint8_t>& mask) const;

	Array<ExtMeshWedge> ExtWedges;
	Array<vec3> Points;
	Array<RefSkeletonBone> RefSkeleton;
	Array<BoneWeightIndex> BoneWeightIndices;
	Array<BoneWeight> BoneWeights;
	Array<vec3> LocalPoints;

	uint32_t SkeletalDepth = 0;
	UAnimation* DefaultAnimation = nullptr;
	uint32_t WeaponBoneIndex = 0;

	struct
	{
		vec3 Origin;
		vec3 XAxis;
		vec3 YAxis;
		vec3 ZAxis;
	} WeaponAdjust;
private:
	// Maps each mesh bone to the animation's bone with the same name (-1 if the animation does not drive it)
	const Array<int>& GetBoneLinks(UAnimation* anim);

	UAnimation* linkedAnim = nullptr;
	Array<int> boneLinks;
};
