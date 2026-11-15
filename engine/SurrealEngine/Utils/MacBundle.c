// Plain C so CoreFoundation's Point/Rect/Size types don't meet the engine's own (via the C++ precompiled header).
#include "MacBundle.h"

#ifdef __APPLE__
#include <CoreFoundation/CoreFoundation.h>

static CFTypeRef GetInfoValue(const char* key)
{
	CFBundleRef bundle = CFBundleGetMainBundle();
	if (!bundle)
		return NULL;
	CFStringRef cfkey = CFStringCreateWithCString(NULL, key, kCFStringEncodingUTF8);
	CFTypeRef value = CFBundleGetValueForInfoDictionaryKey(bundle, cfkey);
	CFRelease(cfkey);
	return value;
}

int MacBundleGetString(const char* key, char* buffer, int size)
{
	CFTypeRef value = GetInfoValue(key);
	if (!value || CFGetTypeID(value) != CFStringGetTypeID() || CFStringGetLength((CFStringRef)value) == 0)
		return 0;
	return CFStringGetCString((CFStringRef)value, buffer, size, kCFStringEncodingUTF8) ? 1 : 0;
}

int MacBundleGetBool(const char* key)
{
	CFTypeRef value = GetInfoValue(key);
	if (!value)
		return 0;
	if (CFGetTypeID(value) == CFBooleanGetTypeID())
		return CFBooleanGetValue((CFBooleanRef)value) ? 1 : 0;
	if (CFGetTypeID(value) == CFStringGetTypeID())
		return CFStringCompare((CFStringRef)value, CFSTR("true"), kCFCompareCaseInsensitive) == kCFCompareEqualTo ||
			CFStringCompare((CFStringRef)value, CFSTR("yes"), kCFCompareCaseInsensitive) == kCFCompareEqualTo;
	return 0;
}

int MacBundleGetPreferredLanguage(char* buffer, int size)
{
	int result = 0;
	CFArrayRef languages = CFLocaleCopyPreferredLanguages();
	if (languages && CFArrayGetCount(languages) > 0)
		result = CFStringGetCString((CFStringRef)CFArrayGetValueAtIndex(languages, 0), buffer, size, kCFStringEncodingUTF8) ? 1 : 0;
	if (languages)
		CFRelease(languages);
	return result;
}

void MacBundleIgnorePersistentState(void)
{
	CFPreferencesSetAppValue(CFSTR("ApplePersistenceIgnoreState"), kCFBooleanTrue, kCFPreferencesCurrentApplication);
	CFPreferencesAppSynchronize(kCFPreferencesCurrentApplication);
}

#else

int MacBundleGetString(const char* key, char* buffer, int size) { (void)key; (void)buffer; (void)size; return 0; }
int MacBundleGetBool(const char* key) { (void)key; return 0; }
int MacBundleGetPreferredLanguage(char* buffer, int size) { (void)buffer; (void)size; return 0; }
void MacBundleIgnorePersistentState(void) {}

#endif
