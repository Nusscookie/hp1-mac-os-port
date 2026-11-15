
#include "Precomp.h"
#include "InstallerWindow.h"
#include <surrealwidgets/core/timer.h>
#include <surrealwidgets/widgets/textlabel/textlabel.h>
#include <surrealwidgets/widgets/pushbutton/pushbutton.h>
#include <surrealwidgets/widgets/dropdown/dropdown.h>
#include <surrealwidgets/widgets/progressbar/progressbar.h>
#include <surrealwidgets/systemdialogs/open_folder_dialog.h>
#include <cstdlib>

static std::string ShortPath(const std::string& path)
{
	const char* home = getenv("HOME");
	if (home && *home && path.rfind(home, 0) == 0)
		return "~" + path.substr(strlen(home));
	return path;
}

bool InstallerWindow::ExecModal(const std::string& gameFolder)
{
	Size screenSize = GetScreenSize();
	double windowWidth = 680.0;
	double windowHeight = 400.0;

	auto window = std::make_unique<InstallerWindow>(gameFolder);
	window->SetFrameGeometry((screenSize.width - windowWidth) * 0.5, (screenSize.height - windowHeight) * 0.5, windowWidth, windowHeight);
	window->Show();

	DisplayWindow::RunLoop();

	return window->installed;
}

InstallerWindow::InstallerWindow(const std::string& gameFolder) : Widget(nullptr, WidgetType::Window), gameFolder(gameFolder)
{
	SetWindowTitle("Install Harry Potter");
	SetWindowBackground(Colorf::fromRgba8(51, 51, 51));
	SetWindowBorderColor(Colorf::fromRgba8(51, 51, 51));
	SetWindowCaptionColor(Colorf::fromRgba8(33, 33, 33));
	SetWindowCaptionTextColor(Colorf::fromRgba8(226, 223, 219));

	Title = new TextLabel(this);
	Title->SetText("Harry Potter and the Philosopher's Stone (PC, 2001)");

	for (const std::string& line : {
		std::string("Insert your original PC CD (Philosopher's or Sorcerer's Stone)."),
		std::string("The game data is copied once to " + ShortPath(gameFolder) + "."),
		std::string("Only data files are copied. The Windows program on the disc is not used."),
		std::string("You can also choose a folder with a copy of the CD or a Windows installation.") })
	{
		TextLabel* label = new TextLabel(this);
		label->SetText(line);
		InfoLines.push_back(label);
	}

	Status = new TextLabel(this);
	Status->SetText("Waiting for the CD...");

	LanguageLabel = new TextLabel(this);
	LanguageLabel->SetText("Language:");
	LanguageList = new Dropdown(this);

	Bar = new ProgressBar(this);
	Bar->SetMin(0);
	Bar->SetMax(1000);
	Bar->SetValue(0);

	ChooseButton = new PushButton(this);
	ChooseButton->SetText("Choose Folder...");
	ChooseButton->OnClick = [this]() { OnChooseFolder(); };

	QuitButton = new PushButton(this);
	QuitButton->SetText("Quit");
	QuitButton->OnClick = [this]() { OnQuit(); };

	InstallButton = new PushButton(this);
	InstallButton->SetText("Install");
	InstallButton->OnClick = [this]() { OnInstall(); };

	LanguageLabel->Hide();
	LanguageList->Hide();
	Bar->Hide();
	InstallButton->SetEnabled(false);

	PollTimer = new Timer(this);
	PollTimer->FuncExpired = [this]() { OnTimer(); };
	PollTimer->Start(250, true);

	ScanForDisc();
}

InstallerWindow::~InstallerWindow()
{
	StopCopy();
}

void InstallerWindow::OnTimer()
{
	if (copying)
	{
		UpdateProgress();
		return;
	}

	// Look for the disc about once a second until one is found or a folder was chosen.
	static int ticks = 0;
	if (!userChoseFolder && (++ticks % 4) == 0)
		ScanForDisc();
}

void InstallerWindow::ScanForDisc()
{
	std::vector<DiscInstall::Source> discs = DiscInstall::FindDiscs();
	if (!discs.empty())
	{
		if (!hasSource || source.path != discs.front().path)
			SetSource(discs.front());
	}
	else if (hasSource)
	{
		hasSource = false;
		Status->SetText("Waiting for the CD...");
		LanguageLabel->Hide();
		LanguageList->Hide();
		InstallButton->SetEnabled(false);
	}
}

void InstallerWindow::SetSource(const DiscInstall::Source& newSource)
{
	source = newSource;
	hasSource = true;
	Status->SetText("Found the game on " + source.label + ". Click Install to copy it (about 560 MB).");

	LanguageList->ClearItems();
	for (const DiscInstall::Language& lang : source.languages)
		LanguageList->AddItem(lang.name);
	if (!source.languages.empty())
	{
		LanguageList->SetSelectedItem(DiscInstall::DefaultLanguageIndex(source));
		LanguageLabel->Show();
		LanguageList->Show();
	}
	else
	{
		LanguageLabel->Hide();
		LanguageList->Hide();
	}
	InstallButton->SetEnabled(true);
	OnGeometryChanged();

	// Test aid (like SURREAL_NO_CRASH_WINDOW): press Install as soon as a disc is found.
	if (getenv("SURREAL_INSTALLER_AUTOINSTALL"))
		OnInstall();
}

void InstallerWindow::OnChooseFolder()
{
	auto dialog = OpenFolderDialog::Create(this);
	dialog->SetTitle("Choose the CD or a folder with the Harry Potter game files");
	if (!dialog->Show())
		return;

	DiscInstall::Source chosen;
	if (DiscInstall::Probe(dialog->SelectedPath(), chosen))
	{
		userChoseFolder = true;
		SetSource(chosen);
	}
	else
	{
		Status->SetText("That folder does not contain the game (System/HPBase.u is missing).");
	}
}

void InstallerWindow::OnInstall()
{
	if (!hasSource || copying)
		return;

	std::string languageCode;
	int selected = LanguageList->GetSelectedItem();
	if (selected >= 0 && selected < (int)source.languages.size())
		languageCode = source.languages[selected].code;

	copying = true;
	copyFinished = false;
	copyError.clear();
	progress.cancel = false;
	progress.bytesDone = 0;
	progress.bytesTotal = 0;

	InstallButton->SetEnabled(false);
	ChooseButton->SetEnabled(false);
	LanguageList->SetEnabled(false);
	QuitButton->SetText("Cancel");
	Bar->SetValue(0);
	Bar->Show();
	Status->SetText("Copying...");

	DiscInstall::Source src = source;
	std::string dest = gameFolder;
	copyThread = std::thread([this, src, languageCode, dest]() {
		try
		{
			DiscInstall::Install(src, languageCode, dest, progress);
		}
		catch (const std::exception& e)
		{
			copyError = e.what();
		}
		copyFinished = true;
	});
}

void InstallerWindow::UpdateProgress()
{
	uint64_t total = progress.bytesTotal;
	uint64_t done = progress.bytesDone;
	if (total > 0)
	{
		Bar->SetValue((int)(done * 1000 / total));
		Status->SetText("Copying from " + source.label + "... " + std::to_string(done / 1000000) + " of " + std::to_string(total / 1000000) + " MB");
	}

	if (copyFinished)
	{
		copyThread.join();
		copying = false;
		if (copyError.empty() && DiscInstall::IsInstalled(gameFolder))
		{
			installed = true;
			PollTimer->Stop();
			DisplayWindow::ExitLoop();
			return;
		}

		Status->SetText(copyError.empty() ? std::string("Copying did not finish.") : copyError);
		Bar->Hide();
		QuitButton->SetText("Quit");
		InstallButton->SetText("Try Again");
		InstallButton->SetEnabled(hasSource);
		ChooseButton->SetEnabled(true);
		LanguageList->SetEnabled(true);
	}
}

void InstallerWindow::StopCopy()
{
	if (copyThread.joinable())
	{
		progress.cancel = true;
		copyThread.join();
	}
	copying = false;
}

void InstallerWindow::OnQuit()
{
	if (copying)
	{
		// Cancel: what was copied stays and is reused by the next attempt.
		progress.cancel = true;
		return;
	}
	StopCopy();
	DisplayWindow::ExitLoop();
}

void InstallerWindow::OnClose()
{
	StopCopy();
	DisplayWindow::ExitLoop();
}

void InstallerWindow::OnGeometryChanged()
{
	double w = GetWidth();
	double h = GetHeight();
	double x = 24.0;
	double y = 20.0;
	double lineHeight = Title->GetPreferredHeight();

	Title->SetFrameGeometry(x, y, w - 2 * x, lineHeight);
	y += lineHeight + 14.0;
	for (TextLabel* label : InfoLines)
	{
		label->SetFrameGeometry(x, y, w - 2 * x, lineHeight);
		y += lineHeight + 2.0;
	}
	y += 16.0;
	Status->SetFrameGeometry(x, y, w - 2 * x, lineHeight);
	y += lineHeight + 14.0;

	double dropdownHeight = LanguageList->GetPreferredHeight();
	LanguageLabel->SetFrameGeometry(x, y + (dropdownHeight - lineHeight) * 0.5, 90.0, lineHeight);
	LanguageList->SetFrameGeometry(x + 95.0, y, 200.0, dropdownHeight);
	y += dropdownHeight + 14.0;
	Bar->SetFrameGeometry(x, y, w - 2 * x, 20.0);

	double buttonHeight = InstallButton->GetPreferredHeight() + InstallButton->GetNoncontentTop() + InstallButton->GetNoncontentBottom();
	double by = h - 20.0 - buttonHeight;
	ChooseButton->SetFrameGeometry(x, by, 170.0, buttonHeight);
	InstallButton->SetFrameGeometry(w - x - 120.0, by, 120.0, buttonHeight);
	QuitButton->SetFrameGeometry(w - x - 250.0, by, 120.0, buttonHeight);
}
