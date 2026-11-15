#pragma once

// Copies the Harry Potter 1 game data from the original PC CD (or a folder holding a copy of
// it, or an existing Windows installation) into a game folder the engine can run.
// Only data packages, localisation text and ini files are copied. The Windows executables,
// drivers, installers and copy-protection files on the disc are never read.
// Same rules as tools/install_from_cd.py.

#include <atomic>
#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace DiscInstall
{
	struct Language
	{
		std::string folder; // language subfolder on the disc ("0", "1", ...)
		std::string code;   // Language= value from its Default.ini ("eng", "ger", ...)
		std::string name;   // display name ("English", "Deutsch", ...)
	};

	struct Source
	{
		std::filesystem::path path;
		std::string label; // volume or folder name
		std::vector<Language> languages; // empty when the source has a single, flat System folder
	};

	struct Progress
	{
		std::atomic<uint64_t> bytesDone{ 0 };
		std::atomic<uint64_t> bytesTotal{ 0 };
		std::atomic<bool> cancel{ false };
	};

	// True if the folder holds the game's script packages.
	bool LooksLikeGame(const std::filesystem::path& folder);

	// Reads the language folders of a disc or copy. Returns false if it is not the game.
	bool Probe(const std::filesystem::path& folder, Source& source);

	// Mounted volumes (/Volumes on macOS) that hold the game.
	std::vector<Source> FindDiscs();

	// A finished installation: game data present and no interrupted copy.
	bool IsInstalled(const std::filesystem::path& gameFolder);

	// Copies the data. languageCode picks Default.ini when the source has language folders
	// (empty: English if present, else the first). Throws std::runtime_error on failure or cancel.
	void Install(const Source& source, const std::string& languageCode, const std::filesystem::path& gameFolder, Progress& progress);

	// Index into source.languages that matches the user's system language, or 0.
	int DefaultLanguageIndex(const Source& source);
}
