
#include "Precomp.h"
#include "IcoImage.h"
#include "Utils/File.h"
#include <surrealwidgets/core/image.h>

static uint16_t Read16(const uint8_t* p) { return (uint16_t)(p[0] | (p[1] << 8)); }
static uint32_t Read32(const uint8_t* p) { return (uint32_t)(p[0] | (p[1] << 8) | (p[2] << 16) | ((uint32_t)p[3] << 24)); }

std::shared_ptr<Image> LoadIcoImage(const std::string& filename, int size)
{
	try
	{
		Array<uint8_t> data = File::read_all_bytes(filename);
		if (data.size() < 6 || Read16(&data[2]) != 1)
			return nullptr;

		// Pick the largest entry
		int count = Read16(&data[4]);
		int w = 0, h = 0;
		uint32_t offset = 0;
		for (int i = 0; i < count; i++)
		{
			size_t e = 6 + 16 * (size_t)i;
			if (e + 16 > data.size())
				return nullptr;
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
			return nullptr;

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
			return nullptr;

		std::vector<uint32_t> source((size_t)w * h);
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

		std::vector<uint32_t> scaled((size_t)size * size);
		for (int y = 0; y < size; y++)
			for (int x = 0; x < size; x++)
				scaled[(size_t)y * size + x] = source[(size_t)(y * h / size) * w + (x * w / size)];

		return Image::Create(size, size, ImageFormat::R8G8B8A8, scaled.data());
	}
	catch (...)
	{
		return nullptr;
	}
}
