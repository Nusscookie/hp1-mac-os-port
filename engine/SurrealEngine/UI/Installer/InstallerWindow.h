#pragma once

#include <surrealwidgets/core/widget.h>
#include "DiscInstall.h"
#include <memory>
#include <thread>

class TextLabel;
class PushButton;
class Dropdown;
class ProgressBar;
class Timer;

// First-launch window of the HP1 app: waits for the game CD (or a folder chosen by the user),
// then copies the game data into the app's game folder.
class InstallerWindow : public Widget
{
public:
	// Returns true once the game data is installed in gameFolder, false if the user quit.
	static bool ExecModal(const std::string& gameFolder);

	InstallerWindow(const std::string& gameFolder);
	~InstallerWindow();

protected:
	void OnClose() override;
	void OnGeometryChanged() override;

private:
	void OnTimer();
	void ScanForDisc();
	void SetSource(const DiscInstall::Source& source);
	void OnChooseFolder();
	void OnInstall();
	void OnQuit();
	void UpdateProgress();
	void StopCopy();

	std::string gameFolder;

	TextLabel* Title = nullptr;
	std::vector<TextLabel*> InfoLines;
	TextLabel* Status = nullptr;
	TextLabel* LanguageLabel = nullptr;
	Dropdown* LanguageList = nullptr;
	ProgressBar* Bar = nullptr;
	PushButton* ChooseButton = nullptr;
	PushButton* QuitButton = nullptr;
	PushButton* InstallButton = nullptr;
	Timer* PollTimer = nullptr;

	bool hasSource = false;
	bool userChoseFolder = false;
	DiscInstall::Source source;

	std::thread copyThread;
	DiscInstall::Progress progress;
	std::atomic<bool> copyFinished{ false };
	bool copying = false;
	std::string copyError; // written by the copy thread before copyFinished is set

	bool installed = false;
};
