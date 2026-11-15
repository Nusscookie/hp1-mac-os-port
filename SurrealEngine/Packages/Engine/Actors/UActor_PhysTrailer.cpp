
#include "Precomp.h"
#include "UActor.h"
#include "Engine.h"

void UActor::TickTrailer(float elapsed)
{
	if (!Owner())
		return;

	vec3 newLocation = Owner()->Location();

	// Harry Potter 1: AttachToOwner(bone) stores bone index + 1 in AnimBone. The actor then follows that bone.
	if (engine->LaunchInfo.IsHarryPotter1() && AnimBone() > 0)
	{
		Array<mat4> bones;
		size_t bone = (size_t)AnimBone() - 1;
		if (Owner()->GetSkeletalBoneMatrices(bones) && bone < bones.size())
		{
			vec4 p = Owner()->GetMeshToWorldMatrix() * (bones[bone] * vec4(0.0f, 0.0f, 0.0f, 1.0f));
			SetLocation(vec3(p.x, p.y, p.z));
			return;
		}
	}

	if (engine->LaunchInfo.ue1Version >= 400 && bTrailerPrePivot())
	{
		newLocation += PrePivot();
	}

	SetLocation(newLocation);

	if ((engine->LaunchInfo.ue1Version < 400 || bTrailerSameRotation()) && DrawType() != DT_Sprite)
	{
		SetRotation(Owner()->Rotation());
	}
}
