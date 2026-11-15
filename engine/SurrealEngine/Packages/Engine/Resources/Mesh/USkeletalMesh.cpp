
#include "Precomp.h"
#include "USkeletalMesh.h"
#include "Packages/Engine/Resources/Mesh/UAnimation.h"

void USkeletalMesh::Load(ObjectStream* stream)
{
	ULodMesh::Load(stream);

	int NumExtWedges = stream->ReadIndex();
	for (int i = 0; i < NumExtWedges; i++)
	{
		ExtMeshWedge wedge;
		wedge.Vertex = stream->ReadUInt16();
		wedge.Flags = stream->ReadUInt16();
		wedge.U = stream->ReadFloat();
		wedge.V = stream->ReadFloat();
		ExtWedges.push_back(wedge);
	}

	int NumPoints = stream->ReadIndex();
	for (int i = 0; i < NumPoints; i++)
	{
		vec3 p;
		p.x = stream->ReadFloat();
		p.y = stream->ReadFloat();
		p.z = stream->ReadFloat();
		Points.push_back(p);
	}

	int NumRefSkeletonBones = stream->ReadIndex();
	for (int i = 0; i < NumRefSkeletonBones; i++)
	{
		RefSkeletonBone bone;
		bone.Name = stream->ReadName();
		bone.Flags = stream->ReadUInt32();
		bone.Orientation.x = stream->ReadFloat();
		bone.Orientation.y = stream->ReadFloat();
		bone.Orientation.z = stream->ReadFloat();
		bone.Orientation.w = stream->ReadFloat();
		bone.Position.x = stream->ReadFloat();
		bone.Position.y = stream->ReadFloat();
		bone.Position.z = stream->ReadFloat();
		bone.Length = stream->ReadFloat();
		bone.Size.x = stream->ReadFloat();
		bone.Size.y = stream->ReadFloat();
		bone.Size.z = stream->ReadFloat();
		bone.NumChildren = stream->ReadUInt32();
		bone.ParentIndex = stream->ReadUInt32();
		RefSkeleton.push_back(bone);
	}

	int NumBoneWeightIndices = stream->ReadIndex();
	for (int i = 0; i < NumBoneWeightIndices; i++)
	{
		BoneWeightIndex index;
		index.WeightIndex = stream->ReadUInt16();
		index.Number = stream->ReadUInt16();
		index.DetailA = stream->ReadUInt16();
		index.DetailB = stream->ReadUInt16();
		BoneWeightIndices.push_back(index);
	}

	int NumBoneWeights = stream->ReadIndex();
	for (int i = 0; i < NumBoneWeights; i++)
	{
		BoneWeight weight;
		weight.PointIndex = stream->ReadUInt16();
		weight.BoneWeight = stream->ReadUInt16();
		BoneWeights.push_back(weight);
	}

	int NumLocalPoints = stream->ReadIndex();
	for (int i = 0; i < NumLocalPoints; i++)
	{
		vec3 p;
		p.x = stream->ReadFloat();
		p.y = stream->ReadFloat();
		p.z = stream->ReadFloat();
		LocalPoints.push_back(p);
	}

	SkeletalDepth = stream->ReadUInt32();
	DefaultAnimation = stream->ReadObject<UAnimation>();
	WeaponBoneIndex = stream->ReadUInt32();

	WeaponAdjust.Origin.x = stream->ReadFloat();
	WeaponAdjust.Origin.y = stream->ReadFloat();
	WeaponAdjust.Origin.z = stream->ReadFloat();
	WeaponAdjust.XAxis.x = stream->ReadFloat();
	WeaponAdjust.XAxis.y = stream->ReadFloat();
	WeaponAdjust.XAxis.z = stream->ReadFloat();
	WeaponAdjust.YAxis.x = stream->ReadFloat();
	WeaponAdjust.YAxis.y = stream->ReadFloat();
	WeaponAdjust.YAxis.z = stream->ReadFloat();
	WeaponAdjust.ZAxis.x = stream->ReadFloat();
	WeaponAdjust.ZAxis.y = stream->ReadFloat();
	WeaponAdjust.ZAxis.z = stream->ReadFloat();

	// Skeletal meshes have no vertex animation frames. Their sequences are kept in the animation object.
	if (AnimSeqs.empty() && DefaultAnimation)
	{
		DefaultAnimation->LoadNow();
		AnimSeqs = DefaultAnimation->AnimSeqs;
	}
}

int USkeletalMesh::FindBone(const NameString& name) const
{
	for (size_t i = 0; i < RefSkeleton.size(); i++)
		if (RefSkeleton[i].Name == name)
			return (int)i;
	return -1;
}

void USkeletalMesh::GetRefPose(SkeletalPose& pose) const
{
	size_t count = RefSkeleton.size();
	pose.Rotations.resize(count);
	pose.Positions.resize(count);
	for (size_t i = 0; i < count; i++)
	{
		pose.Rotations[i] = RefSkeleton[i].Orientation;
		pose.Positions[i] = RefSkeleton[i].Position;
	}
}

const Array<int>& USkeletalMesh::GetBoneLinks(UAnimation* anim)
{
	if (linkedAnim != anim || boneLinks.size() != RefSkeleton.size())
	{
		linkedAnim = anim;
		boneLinks.resize(RefSkeleton.size());
		for (size_t i = 0; i < RefSkeleton.size(); i++)
			boneLinks[i] = anim ? anim->FindBone(RefSkeleton[i].Name) : -1;
	}
	return boneLinks;
}

static quaternion BlendRotation(const quaternion& a, quaternion b, float alpha)
{
	// Take the short way around, then normalized lerp. Keys are dense enough that this is indistinguishable from slerp.
	if (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w < 0.0f)
	{
		b.x = -b.x; b.y = -b.y; b.z = -b.z; b.w = -b.w;
	}
	quaternion q;
	q.x = a.x + (b.x - a.x) * alpha;
	q.y = a.y + (b.y - a.y) * alpha;
	q.z = a.z + (b.z - a.z) * alpha;
	q.w = a.w + (b.w - a.w) * alpha;
	float len = std::sqrt(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
	if (len > 0.000001f)
	{
		float inv = 1.0f / len;
		q.x *= inv; q.y *= inv; q.z *= inv; q.w *= inv;
	}
	else
	{
		q = a;
	}
	return q;
}

void USkeletalMesh::GetAnimPose(UAnimation* anim, int moveIndex, float time, SkeletalPose& pose)
{
	GetRefPose(pose);

	if (!anim || moveIndex < 0 || (size_t)moveIndex >= anim->Moves.size())
		return;

	const AnimMove& move = anim->Moves[moveIndex];
	const Array<int>& links = GetBoneLinks(anim);

	for (size_t i = 0; i < links.size(); i++)
	{
		int animBone = links[i];
		if (animBone < 0 || (size_t)animBone >= move.AnimTracks.size())
			continue;

		const AnimTrack& track = move.AnimTracks[animBone];
		if (track.KeyQuat.empty())
			continue;

		// Find the two keys surrounding the time
		size_t key0 = 0, key1 = 0;
		float alpha = 0.0f;
		size_t numTimes = std::min(track.KeyTime.size(), track.KeyQuat.size());
		if (numTimes > 1)
		{
			key1 = 1;
			while (key1 + 1 < numTimes && track.KeyTime[key1] <= time)
				key1++;
			key0 = key1 - 1;
			float t0 = track.KeyTime[key0];
			float t1 = track.KeyTime[key1];
			alpha = (t1 > t0) ? clamp((time - t0) / (t1 - t0), 0.0f, 1.0f) : 0.0f;
		}

		pose.Rotations[i] = (key0 == key1) ? track.KeyQuat[key0] : BlendRotation(track.KeyQuat[key0], track.KeyQuat[key1], alpha);

		if (track.KeyPos.size() == 1)
			pose.Positions[i] = track.KeyPos[0];
		else if (key1 < track.KeyPos.size())
			pose.Positions[i] = mix(track.KeyPos[key0], track.KeyPos[key1], alpha);
	}
}

void USkeletalMesh::BlendPose(SkeletalPose& pose, const SkeletalPose& target, float alpha, const Array<uint8_t>* boneMask)
{
	size_t count = std::min(pose.Rotations.size(), target.Rotations.size());
	for (size_t i = 0; i < count; i++)
	{
		if (boneMask && (i >= boneMask->size() || !(*boneMask)[i]))
			continue;
		pose.Rotations[i] = BlendRotation(pose.Rotations[i], target.Rotations[i], alpha);
		pose.Positions[i] = mix(pose.Positions[i], target.Positions[i], alpha);
	}
}

void USkeletalMesh::GetBoneMatrices(const SkeletalPose& pose, Array<mat4>& bones) const
{
	size_t count = RefSkeleton.size();
	bones.resize(count);
	for (size_t i = 0; i < count; i++)
	{
		// The stored quaternions rotate the other way round compared to mat4::quaternion
		const quaternion& q = pose.Rotations[i];
		mat4 local = mat4::translate(pose.Positions[i]) * mat4::quaternion(-q.x, -q.y, -q.z, q.w);

		uint32_t parent = RefSkeleton[i].ParentIndex;
		if (i == 0 || parent >= i)
			bones[i] = local;
		else
			bones[i] = bones[parent] * local;
	}
}

void USkeletalMesh::SkinVertices(const Array<mat4>& bones, Array<vec3>& verts, Array<vec3>& normals) const
{
	verts.assign(Points.size(), vec3(0.0f));

	size_t numBones = std::min(BoneWeightIndices.size(), bones.size());
	for (size_t b = 0; b < numBones; b++)
	{
		const BoneWeightIndex& range = BoneWeightIndices[b];
		const mat4& bone = bones[b];
		size_t end = std::min((size_t)range.WeightIndex + range.Number, std::min(BoneWeights.size(), LocalPoints.size()));
		for (size_t w = range.WeightIndex; w < end; w++)
		{
			const BoneWeight& weight = BoneWeights[w];
			if (weight.PointIndex >= verts.size())
				continue;
			vec4 p = bone * vec4(LocalPoints[w], 1.0f);
			verts[weight.PointIndex] += vec3(p.x, p.y, p.z) * (weight.BoneWeight * (1.0f / 65535.0f));
		}
	}

	normals.assign(verts.size(), vec3(0.0f));
	for (const MeshFace& face : Faces)
	{
		size_t v0 = Wedges[face.Indices[0]].Vertex;
		size_t v1 = Wedges[face.Indices[1]].Vertex;
		size_t v2 = Wedges[face.Indices[2]].Vertex;
		if (v0 >= verts.size() || v1 >= verts.size() || v2 >= verts.size())
			continue;
		vec3 n = cross(verts[v1] - verts[v0], verts[v2] - verts[v0]);
		normals[v0] += n;
		normals[v1] += n;
		normals[v2] += n;
	}
	for (vec3& n : normals)
	{
		float len = length(n);
		n = (len > 0.000001f) ? n * (1.0f / len) : vec3(0.0f, 0.0f, 1.0f);
	}
}

void USkeletalMesh::GetBoneMask(int rootBone, Array<uint8_t>& mask) const
{
	mask.assign(RefSkeleton.size(), 0);
	if (rootBone < 0 || (size_t)rootBone >= mask.size())
		return;
	mask[rootBone] = 1;
	for (size_t i = (size_t)rootBone + 1; i < mask.size(); i++)
	{
		uint32_t parent = RefSkeleton[i].ParentIndex;
		if (parent < i && mask[parent])
			mask[i] = 1;
	}
}

void USkeletalMesh::Save(PackageStreamWriter* stream)
{
	ULodMesh::Save(stream);

	stream->WriteIndex((int)ExtWedges.size());
	for (const ExtMeshWedge& wedge : ExtWedges)
	{
		stream->WriteUInt16(wedge.Vertex);
		stream->WriteUInt16(wedge.Flags);
		stream->WriteFloat(wedge.U);
		stream->WriteFloat(wedge.V);
	}

	stream->WriteIndex((int)Points.size());
	for (const vec3& p : Points)
	{
		stream->WriteFloat(p.x);
		stream->WriteFloat(p.y);
		stream->WriteFloat(p.z);
	}

	stream->WriteIndex((int)RefSkeleton.size());
	for (const RefSkeletonBone& bone : RefSkeleton)
	{
		stream->WriteName(bone.Name);
		stream->WriteUInt32(bone.Flags);
		stream->WriteFloat(bone.Orientation.x);
		stream->WriteFloat(bone.Orientation.y);
		stream->WriteFloat(bone.Orientation.z);
		stream->WriteFloat(bone.Orientation.w);
		stream->WriteFloat(bone.Position.x);
		stream->WriteFloat(bone.Position.y);
		stream->WriteFloat(bone.Position.z);
		stream->WriteFloat(bone.Length);
		stream->WriteFloat(bone.Size.x);
		stream->WriteFloat(bone.Size.y);
		stream->WriteFloat(bone.Size.z);
		stream->WriteUInt32(bone.NumChildren);
		stream->WriteUInt32(bone.ParentIndex);
	}

	stream->WriteIndex((int)BoneWeightIndices.size());
	for (const BoneWeightIndex& index : BoneWeightIndices)
	{
		stream->WriteUInt16(index.WeightIndex);
		stream->WriteUInt16(index.Number);
		stream->WriteUInt16(index.DetailA);
		stream->WriteUInt16(index.DetailB);
	}

	stream->WriteIndex((int)BoneWeights.size());
	for (const BoneWeight& weight : BoneWeights)
	{
		stream->WriteUInt16(weight.PointIndex);
		stream->WriteUInt16(weight.BoneWeight);
	}

	stream->WriteIndex((int)LocalPoints.size());
	for (const vec3& p : LocalPoints)
	{
		stream->WriteFloat(p.x);
		stream->WriteFloat(p.y);
		stream->WriteFloat(p.z);
	}

	stream->WriteUInt32(SkeletalDepth);
	stream->WriteObject(DefaultAnimation);
	stream->WriteUInt32(WeaponBoneIndex);

	stream->WriteFloat(WeaponAdjust.Origin.x);
	stream->WriteFloat(WeaponAdjust.Origin.y);
	stream->WriteFloat(WeaponAdjust.Origin.z);
	stream->WriteFloat(WeaponAdjust.XAxis.x);
	stream->WriteFloat(WeaponAdjust.XAxis.y);
	stream->WriteFloat(WeaponAdjust.XAxis.z);
	stream->WriteFloat(WeaponAdjust.YAxis.x);
	stream->WriteFloat(WeaponAdjust.YAxis.y);
	stream->WriteFloat(WeaponAdjust.YAxis.z);
	stream->WriteFloat(WeaponAdjust.ZAxis.x);
	stream->WriteFloat(WeaponAdjust.ZAxis.y);
	stream->WriteFloat(WeaponAdjust.ZAxis.z);
}
