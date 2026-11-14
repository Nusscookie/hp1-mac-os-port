
#include "Precomp.h"
#include "UAnimation.h"
#include "Engine.h"

void UAnimation::Load(ObjectStream* stream)
{
	UObject::Load(stream);

	if (engine->LaunchInfo.IsHarryPotter1())
	{
		LoadHarryPotter1(stream);
		return;
	}

	if (engine->LaunchInfo.IsHarryPotter2())
		return; // To do: format changed. Figure out how. Maybe the arrays became skippable?

	int NumRefBones = stream->ReadIndex();
	for (int i = 0; i < NumRefBones; i++)
	{
		RefBone refbone;
		refbone.Name = stream->ReadName();
		refbone.Flags = stream->ReadUInt32();
		refbone.ParentIndex = stream->ReadUInt32();
		RefBones.push_back(refbone);
	}

	int NumMoves = stream->ReadIndex();
	for (int i = 0; i < NumMoves; i++)
	{
		AnimMove move;
		move.RootSpeed3D.x = stream->ReadFloat();
		move.RootSpeed3D.y = stream->ReadFloat();
		move.RootSpeed3D.z = stream->ReadFloat();
		move.TrackTime = stream->ReadFloat();
		move.StartBone = stream->ReadUInt32();
		move.Flags = stream->ReadUInt32();

		int NumBoneIndices = stream->ReadIndex();
		for (int j = 0; j < NumBoneIndices; j++)
			move.BoneIndices.push_back(stream->ReadUInt32());

		int NumAnimTracks = stream->ReadIndex();
		for (int j = 0; j < NumAnimTracks; j++)
		{
			AnimTrack track;
			track.Flags = stream->ReadUInt32();

			int NumKeyQuat = stream->ReadIndex();
			for (int k = 0; k < NumKeyQuat; k++)
			{
				quaternion q;
				q.x = stream->ReadFloat();
				q.y = stream->ReadFloat();
				q.z = stream->ReadFloat();
				q.w = stream->ReadFloat();
				track.KeyQuat.push_back(q);
			}

			int NumKeyPos = stream->ReadIndex();
			for (int k = 0; k < NumKeyPos; k++)
			{
				vec3 pos;
				pos.x = stream->ReadFloat();
				pos.y = stream->ReadFloat();
				pos.z = stream->ReadFloat();
				track.KeyPos.push_back(pos);
			}

			int NumKeyTime = stream->ReadIndex();
			for (int k = 0; k < NumKeyTime; k++)
				track.KeyTime.push_back(stream->ReadFloat());

			move.AnimTracks.push_back(track);
		}

		move.RootTrack.Flags = stream->ReadUInt32();

		int NumKeyQuat = stream->ReadIndex();
		for (int k = 0; k < NumKeyQuat; k++)
		{
			quaternion q;
			q.x = stream->ReadFloat();
			q.y = stream->ReadFloat();
			q.z = stream->ReadFloat();
			q.w = stream->ReadFloat();
			move.RootTrack.KeyQuat.push_back(q);
		}

		int NumKeyPos = stream->ReadIndex();
		for (int k = 0; k < NumKeyPos; k++)
		{
			vec3 pos;
			pos.x = stream->ReadFloat();
			pos.y = stream->ReadFloat();
			pos.z = stream->ReadFloat();
			move.RootTrack.KeyPos.push_back(pos);
		}

		int NumKeyTime = stream->ReadIndex();
		for (int k = 0; k < NumKeyTime; k++)
			move.RootTrack.KeyTime.push_back(stream->ReadFloat());

		Moves.push_back(move);
	}
}

// Harry Potter 1 (package version 76) stores animation keys compressed:
//
//   RefBones
//   Moves: RootSpeed3D, TrackTime, StartBone, Flags, BoneIndices, then per track only
//          Flags, key counts (quat, pos, time), a position scale and the seconds per time unit. No root track.
//   AnimSeqs: the sequence list (one move per sequence)
//   Pooled keys for all tracks, in move/track order:
//          rotations as 3 x int16 (stereographic projection of a unit quaternion: xyz / (1 + w))
//          positions as 3 x int16 (fraction of the track's position scale)
//          times as uint8 (delta to the previous key, in time units)
void UAnimation::LoadHarryPotter1(ObjectStream* stream)
{
	struct TrackKeys
	{
		int NumQuat = 0;
		int NumPos = 0;
		int NumTime = 0;
		float PosScale = 0.0f;
		float TimeScale = 0.0f;
	};
	Array<TrackKeys> trackKeys;

	int NumRefBones = stream->ReadIndex();
	for (int i = 0; i < NumRefBones; i++)
	{
		RefBone refbone;
		refbone.Name = stream->ReadName();
		refbone.Flags = stream->ReadUInt32();
		refbone.ParentIndex = stream->ReadUInt32();
		RefBones.push_back(refbone);
	}

	int NumMoves = stream->ReadIndex();
	for (int i = 0; i < NumMoves; i++)
	{
		AnimMove move;
		move.RootSpeed3D.x = stream->ReadFloat();
		move.RootSpeed3D.y = stream->ReadFloat();
		move.RootSpeed3D.z = stream->ReadFloat();
		move.TrackTime = stream->ReadFloat();
		move.StartBone = stream->ReadUInt32();
		move.Flags = stream->ReadUInt32();

		int NumBoneIndices = stream->ReadIndex();
		for (int j = 0; j < NumBoneIndices; j++)
			move.BoneIndices.push_back(stream->ReadUInt32());

		int NumAnimTracks = stream->ReadIndex();
		for (int j = 0; j < NumAnimTracks; j++)
		{
			AnimTrack track;
			track.Flags = stream->ReadUInt32();

			TrackKeys keys;
			keys.NumQuat = stream->ReadIndex();
			keys.NumPos = stream->ReadIndex();
			keys.NumTime = stream->ReadIndex();
			keys.PosScale = stream->ReadFloat();
			keys.TimeScale = stream->ReadFloat();
			trackKeys.push_back(keys);

			move.AnimTracks.push_back(std::move(track));
		}

		Moves.push_back(std::move(move));
	}

	int NumAnimSeqs = stream->ReadIndex();
	for (int i = 0; i < NumAnimSeqs; i++)
	{
		MeshAnimSeq seq;
		seq.Name = stream->ReadName();
		seq.Group = stream->ReadName();
		seq.StartFrame = stream->ReadUInt32();
		seq.NumFrames = stream->ReadUInt32();
		int NumNotifys = stream->ReadIndex();
		for (int j = 0; j < NumNotifys; j++)
		{
			MeshAnimNotify notify;
			notify.Time = stream->ReadFloat();
			notify.Function = stream->ReadName();
			seq.Notifys.push_back(notify);
		}
		seq.Rate = stream->ReadFloat();
		AnimSeqs.push_back(std::move(seq));
	}

	auto readInt16Fraction = [&]() { return (float)stream->ReadInt16() * (1.0f / 32767.0f); };

	int NumQuats = stream->ReadIndex();
	Array<quaternion> quats;
	quats.reserve(NumQuats);
	for (int i = 0; i < NumQuats; i++)
	{
		quaternion q;
		// The three values are the quaternion's stereographic projection, xyz / (1 + w).
		// (Harry's pelvis key is exactly 1/3 on each axis where the reference pose has 0.5, 0.5, 0.5, 0.5.)
		float vx = readInt16Fraction();
		float vy = readInt16Fraction();
		float vz = readInt16Fraction();
		float d = vx * vx + vy * vy + vz * vz;
		float s = 2.0f / (1.0f + d);
		q.x = vx * s;
		q.y = vy * s;
		q.z = vz * s;
		q.w = (1.0f - d) / (1.0f + d);
		quats.push_back(q);
	}

	int NumPositions = stream->ReadIndex();
	Array<vec3> positions;
	positions.reserve(NumPositions);
	for (int i = 0; i < NumPositions; i++)
	{
		vec3 pos;
		pos.x = readInt16Fraction();
		pos.y = readInt16Fraction();
		pos.z = readInt16Fraction();
		positions.push_back(pos);
	}

	int NumTimes = stream->ReadIndex();
	Array<uint8_t> times(NumTimes);
	if (NumTimes > 0)
		stream->ReadBytes(times.data(), NumTimes);

	size_t trackIndex = 0, quatIndex = 0, posIndex = 0, timeIndex = 0;
	for (AnimMove& move : Moves)
	{
		for (AnimTrack& track : move.AnimTracks)
		{
			const TrackKeys& keys = trackKeys[trackIndex++];
			if (quatIndex + keys.NumQuat > quats.size() || posIndex + keys.NumPos > positions.size() || timeIndex + keys.NumTime > times.size())
				Exception::Throw("Animation key pool is smaller than its tracks: " + Name.ToString());

			for (int k = 0; k < keys.NumQuat; k++)
				track.KeyQuat.push_back(quats[quatIndex++]);

			for (int k = 0; k < keys.NumPos; k++)
				track.KeyPos.push_back(positions[posIndex++] * keys.PosScale);

			float time = 0.0f;
			for (int k = 0; k < keys.NumTime; k++)
			{
				time += (float)times[timeIndex++] * keys.TimeScale;
				track.KeyTime.push_back(time);
			}
		}
	}
}

void UAnimation::Save(PackageStreamWriter* stream)
{
	UObject::Save(stream);

	stream->WriteIndex((int)RefBones.size());
	for (const RefBone& refbone : RefBones)
	{
		stream->WriteName(refbone.Name);
		stream->WriteUInt32(refbone.Flags);
		stream->WriteUInt32(refbone.ParentIndex);
	}

	stream->WriteIndex((int)Moves.size());
	for (const AnimMove& move : Moves)
	{
		stream->WriteFloat(move.RootSpeed3D.x);
		stream->WriteFloat(move.RootSpeed3D.y);
		stream->WriteFloat(move.RootSpeed3D.z);
		stream->WriteFloat(move.TrackTime);
		stream->WriteUInt32(move.StartBone);
		stream->WriteUInt32(move.Flags);

		stream->WriteIndex((int)move.BoneIndices.size());
		for (uint32_t index : move.BoneIndices)
			stream->WriteUInt32(index);

		stream->WriteIndex((int)move.AnimTracks.size());
		for (const AnimTrack& track : move.AnimTracks)
		{
			stream->WriteUInt32(track.Flags);

			stream->WriteIndex((int)track.KeyQuat.size());
			for (const quaternion& q : track.KeyQuat)
			{
				stream->WriteFloat(q.x);
				stream->WriteFloat(q.y);
				stream->WriteFloat(q.z);
				stream->WriteFloat(q.w);
			}

			stream->WriteIndex((int)track.KeyPos.size());
			for (const vec3& pos : track.KeyPos)
			{
				stream->WriteFloat(pos.x);
				stream->WriteFloat(pos.y);
				stream->WriteFloat(pos.z);
			}

			stream->WriteIndex((int)track.KeyTime.size());
			for (float v : track.KeyTime)
				stream->WriteFloat(v);
		}

		stream->WriteUInt32(move.RootTrack.Flags);

		stream->WriteIndex((int)move.RootTrack.KeyQuat.size());
		for (const quaternion& q : move.RootTrack.KeyQuat)
		{
			stream->WriteFloat(q.x);
			stream->WriteFloat(q.y);
			stream->WriteFloat(q.z);
			stream->WriteFloat(q.w);
		}

		stream->WriteIndex((int)move.RootTrack.KeyPos.size());
		for (const vec3& pos : move.RootTrack.KeyPos)
		{
			stream->WriteFloat(pos.x);
			stream->WriteFloat(pos.y);
			stream->WriteFloat(pos.z);
		}

		stream->WriteIndex((int)move.RootTrack.KeyTime.size());
		for (float v : move.RootTrack.KeyTime)
			stream->WriteFloat(v);

		Moves.push_back(move);
	}
}
