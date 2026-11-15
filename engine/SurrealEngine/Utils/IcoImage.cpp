
#include "Precomp.h"
#include "IcoImage.h"
#include "Utils/File.h"
#include <surrealwidgets/core/image.h>

static uint16_t Read16(const uint8_t* p) { return (uint16_t)(p[0] | (p[1] << 8)); }
static uint32_t Read32(const uint8_t* p) { return (uint32_t)(p[0] | (p[1] << 8) | (p[2] << 16) | ((uint32_t)p[3] << 24)); }

// Decodes the largest image into RGBA (premultiplied), returns false on failure.
static bool DecodeIco(const std::string& filename, std::vector<uint32_t>& source, int& w, int& h)
{
	try
	{
		Array<uint8_t> data = File::read_all_bytes(filename);
		if (data.size() < 6 || Read16(&data[2]) != 1)
			return false;

		// Pick the largest entry
		int count = Read16(&data[4]);
		w = 0;
		h = 0;
		uint32_t offset = 0;
		for (int i = 0; i < count; i++)
		{
			size_t e = 6 + 16 * (size_t)i;
			if (e + 16 > data.size())
				return false;
			int ew = data[e] ? data[e] : 256;
			int eh = data[e + 1] ? data[e + 1] : 256;
			if (ew > w)
			{
				w = ew;
				h = eh;
				offset = Read32(&data[e + 12]);
			}
		}
		if (w == 0 || offset + 40 > data.size() || Read32(&data[offset]) == 0x474e5089) // PNG-compressed entries not handled
			return false;

		uint32_t headerSize = Read32(&data[offset]);
		int bpp = Read16(&data[offset + 14]);
		uint32_t colors = Read32(&data[offset + 32]);
		if (colors == 0 && bpp <= 8)
			colors = 1u << bpp;
		size_t paletteOffset = offset + headerSize;
		size_t xorOffset = paletteOffset + 4 * (size_t)colors;
		size_t xorPitch = (((size_t)w * bpp + 31) / 32) * 4;
		size_t andOffset = xorOffset + xorPitch * h;
		size_t andPitch = (((size_t)w + 31) / 32) * 4;
		bool hasMask = bpp != 32;
		if ((bpp != 1 && bpp != 4 && bpp != 8 && bpp != 32) || andOffset + (hasMask ? andPitch * h : 0) > data.size())
			return false;

		source.assign((size_t)w * h, 0);
		for (int y = 0; y < h; y++)
		{
			int sy = h - 1 - y; // bottom-up rows
			for (int x = 0; x < w; x++)
			{
				uint8_t r, g, b, a;
				if (bpp == 32)
				{
					const uint8_t* p = &data[xorOffset + sy * xorPitch + 4 * x];
					b = p[0]; g = p[1]; r = p[2]; a = p[3];
				}
				else
				{
					size_t bit = (size_t)x * bpp;
					uint8_t byte = data[xorOffset + sy * xorPitch + bit / 8];
					int index = (byte >> (8 - bpp - bit % 8)) & ((1 << bpp) - 1);
					const uint8_t* p = &data[paletteOffset + 4 * index];
					b = p[0]; g = p[1]; r = p[2];
					bool transparent = (data[andOffset + sy * andPitch + x / 8] >> (7 - x % 8)) & 1;
					a = transparent ? 0 : 255;
				}
				if (a != 255) // the macOS icon path expects premultiplied alpha
				{
					r = (uint8_t)(r * a / 255); g = (uint8_t)(g * a / 255); b = (uint8_t)(b * a / 255);
				}
				source[(size_t)y * w + x] = r | (g << 8) | (b << 16) | ((uint32_t)a << 24);
			}
		}

		return true;
	}
	catch (...)
	{
		return false;
	}
}

std::shared_ptr<Image> LoadIcoImage(const std::string& filename, int size)
{
	std::vector<uint32_t> source;
	int w, h;
	if (!DecodeIco(filename, source, w, h))
		return nullptr;

	std::vector<uint32_t> scaled((size_t)size * size);
	for (int y = 0; y < size; y++)
		for (int x = 0; x < size; x++)
			scaled[(size_t)y * size + x] = source[(size_t)(y * h / size) * w + (x * w / size)];
	return Image::Create(size, size, ImageFormat::R8G8B8A8, scaled.data());
}

// Icons without transparency have a solid background baked in: make the border color transparent
// where it touches the edge (flood fill). Must match key_out_background in tools/ico_to_icns.py.
static void KeyOutBackground(std::vector<uint32_t>& art, int w, int h)
{
	for (uint32_t p : art)
	{
		if ((p >> 24) != 255)
			return;
	}
	uint32_t bg = art[0];
	std::vector<std::pair<int, int>> stack;
	for (int x = 0; x < w; x++) { stack.push_back({ x, 0 }); stack.push_back({ x, h - 1 }); }
	for (int y = 0; y < h; y++) { stack.push_back({ 0, y }); stack.push_back({ w - 1, y }); }
	while (!stack.empty())
	{
		auto [x, y] = stack.back();
		stack.pop_back();
		if (x < 0 || y < 0 || x >= w || y >= h || art[(size_t)y * w + x] != bg)
			continue;
		art[(size_t)y * w + x] = 0;
		stack.push_back({ x + 1, y }); stack.push_back({ x - 1, y }); stack.push_back({ x, y + 1 }); stack.push_back({ x, y - 1 });
	}
}

// Apple's icon grid: a 824x824 tile centered on a 1024x1024 canvas. The continuous-corner shape is
// approximated by a superellipse |x|^5 + |y|^5 = 1.
static float SquircleCoverage(float px, float py, float cx, float cy, float half)
{
	// 4x4 supersampling, only near the edge
	auto inside = [&](float x, float y) {
		float dx = std::abs(x - cx) / half, dy = std::abs(y - cy) / half;
		return std::pow(dx, 5.0f) + std::pow(dy, 5.0f) <= 1.0f;
	};
	float dx = std::abs(px + 0.5f - cx) / half, dy = std::abs(py + 0.5f - cy) / half;
	float f = std::pow(dx, 5.0f) + std::pow(dy, 5.0f);
	if (f < 0.97f) return 1.0f;
	if (f > 1.03f) return 0.0f;
	int hits = 0;
	for (int sy = 0; sy < 4; sy++)
		for (int sx = 0; sx < 4; sx++)
			hits += inside(px + (sx + 0.5f) / 4.0f, py + (sy + 0.5f) / 4.0f);
	return hits / 16.0f;
}

std::shared_ptr<Image> LoadIcoAsMacAppIcon(const std::string& filename)
{
	std::vector<uint32_t> source;
	int w, h;
	if (!DecodeIco(filename, source, w, h))
		return nullptr;
	KeyOutBackground(source, w, h);

	const int size = 1024;
	const float center = 512.0f, half = 412.0f;
	int scale = std::max(1, 592 / std::max(w, h)); // pixel art at a whole-number scale, about 72% of the tile
	int artW = w * scale, artH = h * scale;
	int artX = (size - artW) / 2, artY = (size - artH) / 2;

	std::vector<uint32_t> pixels((size_t)size * size);
	for (int y = 0; y < size; y++)
	{
		for (int x = 0; x < size; x++)
		{
			// Soft shadow below the tile
			float sdx = std::abs(x + 0.5f - center) / (half + 6.0f), sdy = std::abs(y + 0.5f - (center + 12.0f)) / (half + 6.0f);
			float sf = std::pow(sdx, 5.0f) + std::pow(sdy, 5.0f);
			float shadow = 0.35f * std::clamp((1.12f - sf) / 0.24f, 0.0f, 1.0f);

			// Tile: vertical night-blue gradient with a faint glow behind the art
			float cover = SquircleCoverage((float)x, (float)y, center, center, half);
			float t = (y - (center - half)) / (2.0f * half);
			float glow = std::max(0.0f, 1.0f - std::hypot(x - center, y - center) / 420.0f);
			float r = 54 * (1 - t) + 14 * t + 40 * glow * glow;
			float g = 48 * (1 - t) + 12 * t + 34 * glow * glow;
			float b = 128 * (1 - t) + 46 * t + 50 * glow * glow;
			float a = 255.0f;

			// Pixel art (premultiplied RGBA) over the tile
			if (x >= artX && x < artX + artW && y >= artY && y < artY + artH)
			{
				uint32_t s = source[(size_t)((y - artY) / scale) * w + (x - artX) / scale];
				float sa = (s >> 24) / 255.0f;
				r = (s & 0xff) + r * (1 - sa);
				g = ((s >> 8) & 0xff) + g * (1 - sa);
				b = ((s >> 16) & 0xff) + b * (1 - sa);
			}

			// Combine tile (with coverage) over shadow, premultiplied
			float outA = cover + shadow * (1 - cover);
			float outR = r * cover, outG = g * cover, outB = b * cover;
			pixels[(size_t)y * size + x] = (uint32_t)std::lround(outR) | ((uint32_t)std::lround(outG) << 8) | ((uint32_t)std::lround(outB) << 16) | ((uint32_t)std::lround(outA * a) << 24);
		}
	}
	return Image::Create(size, size, ImageFormat::R8G8B8A8, pixels.data());
}
