# S.T.A.L.K.E.R. Save Editor

A free save editor for the whole S.T.A.L.K.E.R. series: *Shadow of Chernobyl*, *Clear Sky*, *Call of Pripyat*, their
Enhanced Editions, and *S.T.A.L.K.E.R. 2: Heart of Chornobyl*. Change money, items and gear in a save, and get the
original file back with one button if you do not like the result.

**[Download](#download)** · **[Open in the browser](https://stalker-save-editor.pages.dev)** (nothing to install) ·
**[User guide](docs/GUIDE.md)**

**Українською.** Редактор збережень для всієї серії S.T.A.L.K.E.R.: «Тінь Чорнобиля», «Чисте небо», «Поклик
Прип'яті», їхні Enhanced Edition та S.T.A.L.K.E.R. 2. Редагує гроші, предмети, спорядження і схованки, ставить
виправлення ігор та ігрового компаньйона, перевіряє скрипти. [Завантажити](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest) ·
[відкрити в браузері](https://stalker-save-editor.pages.dev) · **[посібник українською](docs/GUIDE.uk.md)**.

**По-русски.** Редактор сохранений для всей серии S.T.A.L.K.E.R.: «Тень Чернобыля», «Чистое небо», «Зов
Припяти», их Enhanced Edition и S.T.A.L.K.E.R. 2. Правит деньги, предметы, снаряжение и тайники, ставит
исправления игр и игрового компаньона, проверяет скрипты. [Скачать](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest) ·
[открыть в браузере](https://stalker-save-editor.pages.dev) · **[руководство на русском](docs/GUIDE.ru.md)**.

![The main screen: the list of saves on the left, the opened save on the right](docs/images/guide/en/overview.png)

> **Before you start.** The editor makes a backup copy before every change, and that copy is your safety net: so far
> only one edited *Call of Pripyat* save has actually been loaded in a game. The Windows and macOS builds have not
> been started by a person yet. Keep the backups.

## Download

Pick your system. All files come from the [latest release](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest).

| System | File | What to do with it |
|---|---|---|
| **Windows** | [SaveEditor-windows-x86_64-setup.exe](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest/download/SaveEditor-windows-x86_64-setup.exe) | Run it and follow the installer. Then open **S.T.A.L.K.E.R. Save Editor** from the Start menu. |
| Windows, without installing | [SaveEditor-windows-x86_64.zip](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest/download/SaveEditor-windows-x86_64.zip) | Unpack the archive anywhere and run `sse-shell.exe`. |
| **Linux**, any distribution | [SaveEditor-linux-x86_64.AppImage](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest/download/SaveEditor-linux-x86_64.AppImage) | Right-click the file → **Properties** → allow running it as a program. Then double-click it. |
| Linux: Ubuntu, Debian, Mint | `stalker-save-editor_<version>_amd64.deb` on the [release page](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest) | Double-click the file: the app centre opens, press **Install**. Then find **S.T.A.L.K.E.R. Save Editor** in the applications menu. |
| Linux, plain archive | [SaveEditor-linux-x86_64.tar.gz](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest/download/SaveEditor-linux-x86_64.tar.gz) | Unpack it and run `sse-shell`. |
| **macOS**, Apple chip (M1 and newer) | [SaveEditor-macos-arm64.dmg](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest/download/SaveEditor-macos-arm64.dmg) | Open the image and drag **SaveEditor** to Applications. The first time, right-click it → **Open**. |
| macOS, Intel | [SaveEditor-macos-x86_64.dmg](https://github.com/Dmitriy-DE/stalker-save-editor-rs/releases/latest/download/SaveEditor-macos-x86_64.dmg) | The same. |
| **Nothing to install** | [stalker-save-editor.pages.dev](https://stalker-save-editor.pages.dev) | Open the page and press **Open…**. The save is processed inside your browser and is not uploaded anywhere. |

Two warnings you will probably see, and why they are normal:

- **Windows: "Windows protected your PC".** The installer is not signed with a paid certificate. Press
  **More info → Run anyway**.
- **macOS: "cannot be opened because the developer cannot be verified".** The image is not signed by Apple.
  Right-click the app → **Open** → **Open**. On recent macOS versions the same permission is in **System Settings →
  Privacy & Security → Open Anyway**.

Step-by-step installation for every system is in the [guide](docs/GUIDE.md#1-installation).

## First steps

1. **Close the game.** The editor refuses to write a save while the game is running.
2. **Open the editor.** It looks for your saves by itself and lists them on the left under **SAVE LIBRARY**. If the
   list is empty, press **FIND SAVES**, or press **OPEN…** at the top and pick the save file yourself.
3. **Click a save** in the list. The right side shows what is in it: money, health, rank, items.
4. **Change the money.** Go to the **INVENTORY** tab, type a new number in the **Money** field (or press
   **+10 000**). Nothing is written yet: the change waits in a draft, and **UNDO** takes it back.
5. **Press SAVE** (top right). The editor copies the original file to the backup folder, writes the new save and
   reads it back to check it.
6. **Load the save in the game.**

**Where the backups are** and **how to get the original back**: open the **BACKUPS** tab, press **REFRESH**, click
the entry and choose **Restore in place…** (puts the original back) or **Restore to a copy…** (writes it next to the
save under another name), then **Confirm restore**. The files themselves are kept in:

- Windows: `%LOCALAPPDATA%\StalkerSaveEditor\backups`
- Linux and macOS: `~/.local/share/StalkerSaveEditor/backups`

You can choose another folder in **SETTINGS → BACKUPS → BROWSE…**.

## What you can change, game by game

✅ works · 🧪 experimental: the editor writes it and checks the file, but nobody has loaded the result in the game yet ·
❌ not available

| What you change | Shadow of Chernobyl | Clear Sky | Call of Pripyat | SoC Enhanced | CS Enhanced | CoP Enhanced | S.T.A.L.K.E.R. 2 |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| Money | ✅ | ✅ | ✅ | 🧪 | 🧪 | 🧪 | 🧪 |
| Number of rounds in an ammo pack | ✅ | ✅ | ✅ | 🧪 | 🧪 | 🧪 | 🧪 |
| Add an item | ✅ | ✅ | ✅ | 🧪 | 🧪 | 🧪 | ❌ |
| Remove an item | ✅ | ✅ | ✅ | 🧪 | 🧪 | 🧪 | ❌ |
| Condition of weapons and armour | 🧪 | 🧪 | 🧪 | 🧪 | 🧪 | 🧪 | 🧪 |
| Move an item between slot, belt and backpack | 🧪 | 🧪 | 🧪 | ❌ | ❌ | 🧪 | ❌ |
| Upgrades | ❌ | 🧪 | 🧪 | ❌ | 🧪 | 🧪 | ❌ |
| Relations with factions | 🧪 | 🧪 | 🧪 | ❌ | ❌ | ❌ | ❌ |
| Take an item out of a stash | 🧪 | ❌ | 🧪 | ❌ | ❌ | ❌ | 🧪 |
| Move the character to another location | 🧪 | 🧪 | 🧪 | ❌ | ❌ | ❌ | ❌ |

"Works" means the edit matches reference saves byte for byte; it is not a promise that every game version accepts
it. S.T.A.L.K.E.R. 2 saves made by game version 1.0.x open for reading only: load and save them once in the current
game first. The full table with the reasons for every "no" is in [docs/CAPABILITIES.md](docs/CAPABILITIES.md); the
same table is inside the program under **SETTINGS → CAPABILITIES**.

## Safety

- **A copy before every write.** The original save is copied to the backup folder before the editor touches it.
- **A check after every write.** The new save is read back and compared with what was meant to be written. If
  anything is wrong, the original is put back automatically.
- **One-button return.** The **BACKUPS** tab restores the original at any time.

## More than a save editor

**Game fixes.** A catalogue of bug fixes for the game files of Shadow of Chernobyl, Clear Sky, Call of Pripyat and
their Enhanced Editions. You choose a fix and press a button; every fix can be removed again, and the game files
return to what they were. Not checked in a running game yet. → [Guide: game fixes](docs/GUIDE.md#11-game-fixes)

**In-game companion.** An optional mod with a menu inside the game (press **Esc**, then **F1**): heal, repair gear,
set a marker and jump back to it, quick save. It is installed only when you press **INSTALL**, and removed with one
button. Not checked in a running game yet. → [Guide: companion](docs/GUIDE.md#12-the-in-game-companion)

**Browser edition.** The same editor as a web page, for when you cannot or do not want to install anything. The
save stays in your browser; the edited copy is downloaded back to you. → [Guide: browser edition](docs/GUIDE.md#18-the-browser-edition)

Also inside: a save doctor and a game doctor that look for problems, save comparison and history, Steam Cloud files,
Steam achievements, an item encyclopedia, and 15 interface languages.

## Questions and answers

**Linux: I start the editor and nothing appears.**
Versions before 2.0.2 did not start on GNOME with Wayland (the default on current Ubuntu): download the latest
release. For the AppImage, check that running it as a program is allowed in the file's properties. If the window
still does not appear, use the [browser edition](https://stalker-save-editor.pages.dev) and tell us which desktop
you have.

**Windows SmartScreen or my antivirus complains.**
The installer is not signed with a paid certificate, so Windows does not know its publisher. Press **More info → Run
anyway**. If you want to be sure the file is the real one, compare it with the `.sha256` file published next to it
on the release page.

**Where are the backup copies?**
In the **BACKUPS** tab of the editor, and on disk in `%LOCALAPPDATA%\StalkerSaveEditor\backups` (Windows) or
`~/.local/share/StalkerSaveEditor/backups` (Linux, macOS).

**The game does not load the save I edited. What now?**
Open the **BACKUPS** tab, select the entry for that save, press **Restore in place…** and **Confirm restore**: the
original is back. Then please [tell us](https://github.com/Dmitriy-DE/stalker-save-editor-rs/issues) which game and
which change it was. Changes marked 🧪 above are the ones most likely to need this.

**The editor says the save cannot be written.**
Most often the game is still running: close it and press **SAVE** again. If the change is not available for your
game, the message says so; see the table above.

**How do I report a bug?**
Open **SETTINGS → SUPPORT**, press **Save report…** and attach the file to a new
[issue](https://github.com/Dmitriy-DE/stalker-save-editor-rs/issues). The report does not contain your saves.

**How do I update?**
**SETTINGS → UPDATES → CHECK FOR UPDATES**, then **DOWNLOAD UPDATE** and **INSTALL UPDATE**. The editor never
updates itself without you asking. The AppImage does not update itself: download the new file from the release
page.

**Is it official?**
No. This is an unofficial fan project. It is not made, endorsed or supported by GSC Game World.

## Documentation

- [User guide](docs/GUIDE.md) · [Посібник українською](docs/GUIDE.uk.md) · [Руководство на русском](docs/GUIDE.ru.md)
- [What can be edited, in full](docs/CAPABILITIES.md)
- Release notes: [2.0.0](docs/RELEASE_NOTES_2.0.0.md), [2.0.1](docs/RELEASE_NOTES_2.0.1.md),
  [2.0.2](docs/RELEASE_NOTES_2.0.2.md) (English, Russian, Ukrainian)
- [All documentation pages](docs/README.md)

## For developers

The editor is written in Rust with no third-party crates. Everything a contributor needs is on separate pages:

- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): building from source, the crates, running the checks, adding an
  interface string, the command line.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): crate boundaries and how a save is written safely.
- [docs/CAPABILITIES.md](docs/CAPABILITIES.md): the write-capability table as it is in the code.
- [docs/PACKAGING.md](docs/PACKAGING.md) and [docs/RELEASE.md](docs/RELEASE.md): packages, signing, the release
  procedure.
- [PLAN.md](PLAN.md), [TASKS.md](TASKS.md), [ACCEPTANCE.md](ACCEPTANCE.md), [AGENTS.md](AGENTS.md): plan, open
  work, acceptance criteria, and rules for contributors.
