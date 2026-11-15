#pragma once

#include "Packages/Core/UObject.h"

// Native functions that only exist in Harry Potter 1 (KnowWonder's Unreal Engine 1 branch)
class NHarryPotter
{
public:
	static void RegisterFunctions();

	static void CompareGesture(UObject* Self, ScriptArray& InMousePoints, float fAccuracy, float& ReturnValue);
	static void CompareGesturePoint(UObject* Self, const vec3& InMousePoint, float fAccuracy, float& ReturnValue);
	static void GetWind(UObject* Self, const vec3& Loc, vec3& ReturnValue);
	static void FindPath(UObject* Self, UObject* startPoint, const NameString& DestName, UObject*& ReturnValue);
	static void ModifySound(UObject* Self, uint8_t parameter, float value, std::optional<UObject*> Sound, std::optional<uint8_t> Slot, BitfieldBool& ReturnValue);
	static void SaveGameExists(UObject* Self, BitfieldBool& ReturnValue);
};
