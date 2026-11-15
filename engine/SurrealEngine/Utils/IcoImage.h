#pragma once

#include <memory>
#include <string>

class Image;

// Loads the largest image of a Windows .ico file (palette or 32-bit), scaled up to size x size
// with sharp pixels. Returns nullptr if the file is missing or unreadable.
std::shared_ptr<Image> LoadIcoImage(const std::string& filename, int size);
