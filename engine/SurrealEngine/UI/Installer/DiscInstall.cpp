
#include "Precomp.h"
#include "DiscInstall.h"
#include "Utils/MacBundle.h"
#include <algorithm>
#include <fstream>
#include <set>
#include <stdexcept>

namespace fs = std::filesystem;

namespace DiscInstall
{
	static const char* DataDirs[] = { "System", "Maps", "Textures", "Sounds", "Music", "save", "Help" };

	// Interrupted copies leave this file in the game folder.
	static const char* IncompleteMarker = ".install-incomplete";

	static std::string ToLower(std::string s)
	{
		std::transform(s.begin(), s.end(), s.begin(), [](unsigned char c) { return (char)std::tolower(c); });
		return s;
	}

	// Package, localisation and config files. Everything else (exe, dll, sys, installers) stays on the disc.
	static bool Wanted(const fs::path& file)
	{
		static const std::set<std::string> keep = {
			"u", "unr", "utx", "uax", "umx", "int", "ini", "bmp", "wav", "ico",
			"eng", "fre", "ger",
			"eng_uax", "fre_uax", "ger_uax", "eng_utx", "fre_utx", "ger_utx", "hun_utx",
		};
		std::string ext = ToLower(file.extension().string());
		return !ext.empty() && keep.count(ext.substr(1)) != 0;
	}

	static bool IsLanguageFolder(const std::string& name)
	{
		return !name.empty() && std::all_of(name.begin(), name.end(), [](unsigned char c) { return std::isdigit(c); });
	}

	static std::string ReadLanguageCode(const fs::path& ini)
	{
		std::ifstream file(ini);
		std::string line;
		while (std::getline(file, line))
		{
			if (ToLower(line.substr(0, 9)) == "language=")
			{
				std::string value = line.substr(9);
				while (!value.empty() && (value.back() == '\r' || value.back() == ' '))
					value.pop_back();
				return ToLower(value);
			}
		}
		return {};
	}

	static std::string LanguageName(const std::string& code)
	{
		if (code == "eng" || code == "int") return "English";
		if (code == "ger") return "Deutsch";
		if (code == "fre") return "Fran\xc3\xa7" "ais";
		if (code == "ita") return "Italiano";
		if (code == "spa" || code == "esp") return "Espa\xc3\xb1ol";
		if (code == "dut") return "Nederlands";
		if (code == "pol") return "Polski";
		if (code == "hun") return "Magyar";
		return code;
	}

	bool LooksLikeGame(const fs::path& folder)
	{
		std::error_code ec;
		return fs::is_regular_file(folder / "System" / "HPBase.u", ec) && fs::is_regular_file(folder / "System" / "HarryPotter.u", ec);
	}

	bool Probe(const fs::path& folder, Source& source)
	{
		if (!LooksLikeGame(folder))
			return false;

		source = {};
		source.path = folder;
		source.label = folder.filename().string();
		if (source.label.empty())
			source.label = folder.string();

		std::error_code ec;
		for (const auto& entry : fs::directory_iterator(folder / "System", ec))
		{
			std::string name = entry.path().filename().string();
			if (entry.is_directory(ec) && IsLanguageFolder(name) && fs::is_regular_file(entry.path() / "Default.ini", ec))
			{
				Language lang;
				lang.folder = name;
				lang.code = ReadLanguageCode(entry.path() / "Default.ini");
				lang.name = LanguageName(lang.code.empty() ? name : lang.code);
				source.languages.push_back(lang);
			}
		}
		std::sort(source.languages.begin(), source.languages.end(), [](const Language& a, const Language& b) { return std::stoi(a.folder) < std::stoi(b.folder); });
		return true;
	}

	std::vector<Source> FindDiscs()
	{
		std::vector<Source> found;
#ifdef __APPLE__
		std::error_code ec;
		for (const auto& entry : fs::directory_iterator("/Volumes", ec))
		{
			Source source;
			if (entry.is_directory(ec) && Probe(entry.path(), source))
				found.push_back(source);
		}
#endif
		return found;
	}

	bool IsInstalled(const fs::path& gameFolder)
	{
		std::error_code ec;
		return fs::is_regular_file(gameFolder / "System" / "HPBase.u", ec) && !fs::exists(gameFolder / IncompleteMarker, ec);
	}

	int DefaultLanguageIndex(const Source& source)
	{
		char preferred[64] = {};
		std::string lang = MacBundleGetPreferredLanguage(preferred, sizeof(preferred)) ? ToLower(preferred).substr(0, 2) : "en";
		std::string wanted = lang == "de" ? "ger" : lang == "fr" ? "fre" : "eng";
		for (size_t i = 0; i < source.languages.size(); i++)
		{
			if (source.languages[i].code == wanted)
				return (int)i;
		}
		return 0;
	}

	struct CopyItem
	{
		fs::path from, to;
		uint64_t size = 0;
	};

	static void CopyFileChunked(const CopyItem& item, Progress& progress)
	{
		std::ifstream in(item.from, std::ios::binary);
		if (!in)
			throw std::runtime_error("Could not read " + item.from.string() + " (is the disc clean and still inserted?)");
		fs::create_directories(item.to.parent_path());
		fs::path temp = item.to;
		temp += ".part";
		{
			std::ofstream out(temp, std::ios::binary | std::ios::trunc);
			if (!out)
				throw std::runtime_error("Could not write " + temp.string());
			std::vector<char> buffer(1024 * 1024);
			while (in)
			{
				if (progress.cancel)
					throw std::runtime_error("Installation cancelled");
				in.read(buffer.data(), buffer.size());
				std::streamsize n = in.gcount();
				if (n <= 0)
					break;
				out.write(buffer.data(), n);
				if (!out)
					throw std::runtime_error("Could not write " + temp.string() + " (is the disk full?)");
				progress.bytesDone += (uint64_t)n;
			}
			if (in.bad())
				throw std::runtime_error("Could not read " + item.from.string() + " (is the disc clean and still inserted?)");
		}
		fs::rename(temp, item.to);
		fs::permissions(item.to, fs::perms::owner_read | fs::perms::owner_write | fs::perms::group_read | fs::perms::others_read);
	}

	void Install(const Source& source, const std::string& languageCode, const fs::path& gameFolder, Progress& progress)
	{
		std::vector<CopyItem> items;
		auto add = [&](const fs::path& from, const fs::path& to) {
			std::error_code ec;
			uint64_t size = fs::file_size(from, ec);
			if (ec)
				throw std::runtime_error("Could not read " + from.string());
			items.push_back({ from, to, size });
		};

		const Language* language = nullptr;
		for (const Language& lang : source.languages)
		{
			if (lang.code == languageCode)
				language = &lang;
		}
		if (!language && !source.languages.empty())
		{
			language = &source.languages.front();
			for (const Language& lang : source.languages)
			{
				if (lang.code == "eng")
					language = &lang;
			}
		}

		for (const char* dir : DataDirs)
		{
			fs::path base = source.path / dir;
			std::error_code ec;
			if (!fs::is_directory(base, ec))
				continue;

			std::vector<fs::directory_entry> entries;
			for (const auto& entry : fs::directory_iterator(base, ec))
				entries.push_back(entry);
			std::sort(entries.begin(), entries.end(), [](const auto& a, const auto& b) { return a.path() < b.path(); });

			for (const auto& entry : entries)
			{
				std::string name = entry.path().filename().string();
				if (entry.is_regular_file(ec))
				{
					if (Wanted(entry.path()))
						add(entry.path(), gameFolder / dir / name);
				}
				else if (entry.is_directory(ec) && IsLanguageFolder(name))
				{
					// Per-language folders are flattened into the parent, as the original installer does.
					// All languages are installed (their file names differ) except Default.ini, which picks one.
					for (const auto& sub : fs::directory_iterator(entry.path(), ec))
					{
						std::string subname = sub.path().filename().string();
						if (sub.is_regular_file(ec) && ToLower(subname) != "default.ini" && Wanted(sub.path()))
							add(sub.path(), gameFolder / dir / subname);
					}
				}
				else if (entry.is_directory(ec))
				{
					for (const auto& sub : fs::recursive_directory_iterator(entry.path(), ec))
					{
						if (sub.is_regular_file(ec) && Wanted(sub.path()))
							add(sub.path(), gameFolder / dir / fs::relative(sub.path(), base));
					}
				}
			}
		}
		if (language)
			add(source.path / "System" / language->folder / "Default.ini", gameFolder / "System" / "Default.ini");

		uint64_t total = 0;
		for (const CopyItem& item : items)
			total += item.size;
		progress.bytesDone = 0;
		progress.bytesTotal = total;

		fs::create_directories(gameFolder);
		{
			std::ofstream marker(gameFolder / IncompleteMarker);
			marker << "Copying the game data from " << source.path.string() << " did not finish.\n";
		}

		for (const CopyItem& item : items)
		{
			std::error_code ec;
			bool sameFile = fs::exists(item.to, ec) && fs::file_size(item.to, ec) == item.size && item.to.filename() != "Default.ini";
			if (sameFile)
				progress.bytesDone += item.size; // already there from an earlier, interrupted run
			else
				CopyFileChunked(item, progress);
		}

		fs::remove(gameFolder / IncompleteMarker);
	}
}
