
#include "Precomp.h"
#include "Utils/Exception.h"
#include "Utils/Logger.h"
#include "Utils/CommandLine.h"
#include "GameApp.h"
#include "GameFolder.h"
#include "Engine.h"
#include "UI/WidgetResourceData.h"
#include "UI/ErrorWindow/ErrorWindow.h"
#include "UI/Launcher/LauncherWindow.h"
#include "Utils/File.h"
#include <stdexcept>
#include <iostream>
#include "Utils/MacBundle.h"
#include "UI/Installer/DiscInstall.h"
#include "UI/Installer/InstallerWindow.h"

// The game folder of an app bundle built by tools/make_app.sh: SEGameFolder if it was built
// with game data, otherwise ~/Library/Application Support/<SEDataFolderName>. Empty if not such a bundle.
static std::string GetBundleGameFolder()
{
	char value[4096];
	if (MacBundleGetString("SEGameFolder", value, sizeof(value)))
		return value;
	const char* home = getenv("HOME");
	if (home && MacBundleGetString("SEDataFolderName", value, sizeof(value)))
		return (fs::path(home) / "Library" / "Application Support" / value).string();
	return {};
}

// Started without arguments from an app bundle (Finder, Dock), boot straight into the bundle's game,
// installing it from the CD first if needed. Returns false if the user quit the installer.
static bool AddBundleGameFolder(Array<std::string>& args)
{
	if (!args.empty())
		return true;
	std::string folder = GetBundleGameFolder();
	if (folder.empty())
		return true;

	MacBundleIgnorePersistentState();
	if (!DiscInstall::IsInstalled(folder) && !InstallerWindow::ExecModal(folder))
		return false;

	args.push_back("--nolauncher");
	args.push_back(folder);
	return true;
}

// --install-from=<CD or folder> [--language=eng|ger|fre] <game folder>: copy the game data without a window.
static int InstallFromCommandLine()
{
	std::string from = commandline->GetArg("", "--install-from", "");
	if (commandline->GetItems().size() != 1)
	{
		std::cerr << "Usage: --install-from=<CD or folder> [--language=eng|ger|fre] <game folder>" << std::endl;
		return 1;
	}
	DiscInstall::Source source;
	if (!DiscInstall::Probe(from, source))
	{
		std::cerr << from << " does not contain Harry Potter 1 (System/HPBase.u missing)" << std::endl;
		return 1;
	}
	DiscInstall::Progress progress;
	DiscInstall::Install(source, commandline->GetArg("", "--language", ""), commandline->GetItems().front(), progress);
	std::cout << "Installed " << progress.bytesTotal / 1000000 << " MB into " << commandline->GetItems().front() << std::endl;
	return 0;
}

int GameApp::main(Array<std::string> args)
{
	InitWidgetResources("dark");

	// --autostart skips the launcher window, starts the first game found and reports to stderr.
	bool autostart = false;

	if (!AddBundleGameFolder(args))
	{
		DeinitWidgetResources();
		return 0;
	}

	try
	{
		CommandLine cmd(args);
		commandline = &cmd;
		autostart = commandline->HasArg("-a", "--autostart");

		if (ErrorWindow::CheckCrashReporter())
			return 0;

		if (commandline->HasArg("", "--install-from"))
		{
			int result = InstallFromCommandLine();
			DeinitWidgetResources();
			return result;
		}

		if (commandline->HasArg("-h", "--help"))
		{
			std::cout << "SurrealEngine [--url=<mapname>] [--engineversion=X] [--autostart] [--nolauncher] [--maxticks=N] [Path to game folder]\n"
				"SurrealEngine --install-from=<Harry Potter 1 CD or folder> [--language=eng|ger|fre] <game folder>\n";
			return 0;
		}

		int selectedGameIndex = -1;
		if (autostart)
		{
			Logger::Get()->SetCallback([](const LogMessageLine& line) { std::cerr << "[" << line.Time << "] " << line.Source << (line.Source.empty() ? "" : ": ") << line.Text << std::endl; });
			GameFolderSelection::UpdateList();
			if (GameFolderSelection::Games.empty())
			{
				std::cerr << "No supported game found in the given folder" << std::endl;
				return 1;
			}
			selectedGameIndex = 0;
		}
		else if (commandline->HasArg("", "--nolauncher"))
		{
			// App bundles pass their game folder and skip the launcher, but keep the normal error windows.
			GameFolderSelection::UpdateList();
			if (GameFolderSelection::Games.empty())
				throw std::runtime_error("No supported game found in " + (commandline->GetItems().empty() ? std::string("the given folder") : commandline->GetItems().front()) + ". If the game data moved, rebuild the app with tools/make_app.sh <new path>.");
			selectedGameIndex = 0;
		}
		else
		{
			selectedGameIndex = LauncherWindow::ExecModal();
		}
		if (selectedGameIndex >= 0)
		{
			GameLaunchInfo info = GameFolderSelection::GetLaunchInfo(selectedGameIndex);
			Engine engine(info);
			engine.Run();
		}
	}
	catch (const std::exception& e)
	{
		if (autostart)
		{
			std::cerr << "FATAL: " << e.what() << std::endl;
			DeinitWidgetResources();
			return 1;
		}
		ErrorWindow::ExecModal(e.what(), Logger::Get()->GetLog());
	}

	DeinitWidgetResources();
	return 0;
}
