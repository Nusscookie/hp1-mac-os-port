
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

int GameApp::main(Array<std::string> args)
{
	InitWidgetResources("dark");

	// --autostart skips the launcher window, starts the first game found and reports to stderr.
	bool autostart = false;

	try
	{
		CommandLine cmd(args);
		commandline = &cmd;
		autostart = commandline->HasArg("-a", "--autostart");

		if (ErrorWindow::CheckCrashReporter())
			return 0;

		if (commandline->HasArg("-h", "--help"))
		{
			std::cout << "SurrealEngine [--url=<mapname>] [--engineversion=X] [--autostart] [--nolauncher] [--maxticks=N] [Path to game folder]\n";
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
				throw std::runtime_error("No supported game found in the given folder");
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
