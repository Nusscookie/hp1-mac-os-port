#pragma once

#ifdef __cplusplus
extern "C" {
#endif

// Reads a string from the app bundle's Info.plist into buffer. Returns 1 if present; 0 otherwise
// (always 0 outside a macOS app bundle). The HP1 app uses:
//   SEGameFolder      absolute game folder (set by tools/make_app.sh when it was given game data)
//   SEDataFolderName  folder name under ~/Library/Application Support for data installed from the CD
int MacBundleGetString(const char* key, char* buffer, int size);

// Reads a boolean from Info.plist; 0 if missing.
int MacBundleGetBool(const char* key);

// The user's first preferred language, e.g. "de-DE". Returns 0 if unknown.
int MacBundleGetPreferredLanguage(char* buffer, int size);

// Stops macOS from offering to reopen windows after a crash; that dialog blocks the game window.
void MacBundleIgnorePersistentState(void);

#ifdef __cplusplus
}
#endif
