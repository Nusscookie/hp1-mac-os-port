#pragma once

#include <memory>
#include <string>

class Image;

// Loads the largest image of a Windows .ico file (palette or 32-bit), scaled up to size x size
// with sharp pixels. Returns nullptr if the file is missing or unreadable.
std::shared_ptr<Image> LoadIcoImage(const std::string& filename, int size);

// The same icon laid out like a modern macOS app icon (1024x1024): the pixel art drawn sharp on a
// rounded-square ("squircle") night-blue tile with a soft shadow. Must match tools/ico_to_icns.py.
std::shared_ptr<Image> LoadIcoAsMacAppIcon(const std::string& filename);
