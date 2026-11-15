#pragma once

#ifdef __cplusplus
extern "C" {
#endif

// macOS app bundles built by tools/make_app.sh name their game folder in Info.plist (SEGameFolder).
// Copies it into buffer and returns 1 if present; returns 0 elsewhere (and on other platforms).
int MacBundleGetGameFolder(char* buffer, int size);

// Stops macOS from offering to reopen windows after a crash; that dialog blocks the game window.
void MacBundleIgnorePersistentState(void);

#ifdef __cplusplus
}
#endif
