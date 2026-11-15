// Plain C so CoreFoundation's Point/Rect/Size types don't meet the engine's own (via the C++ precompiled header).
#include "MacBundle.h"

#ifdef __APPLE__
#include <CoreFoundation/CoreFoundation.h>

int MacBundleGetGameFolder(char* buffer, int size)
{
	CFBundleRef bundle = CFBundleGetMainBundle();
	if (!bundle)
		return 0;
	CFTypeRef value = CFBundleGetValueForInfoDictionaryKey(bundle, CFSTR("SEGameFolder"));
	if (!value || CFGetTypeID(value) != CFStringGetTypeID())
		return 0;
	return CFStringGetCString((CFStringRef)value, buffer, size, kCFStringEncodingUTF8) ? 1 : 0;
}

void MacBundleIgnorePersistentState(void)
{
	CFPreferencesSetAppValue(CFSTR("ApplePersistenceIgnoreState"), kCFBooleanTrue, kCFPreferencesCurrentApplication);
	CFPreferencesAppSynchronize(kCFPreferencesCurrentApplication);
}

#else

int MacBundleGetGameFolder(char* buffer, int size) { (void)buffer; (void)size; return 0; }
void MacBundleIgnorePersistentState(void) {}

#endif
