# Steam Workshop packages for the Enhanced Editions

The in-game companion for the three Enhanced Editions is published as three Steam Workshop items, one per game:

| Game | Steam app | `--game` code | Archive |
|---|---|---|---|
| Shadow of Chornobyl — EE | 2427410 | `SOC` | `save_editor_companion_soc.pack` |
| Clear Sky — EE | 2427420 | `CS` | `save_editor_companion_cs.pack` |
| Call of Prypiat — EE | 2427430 | `COP` | `save_editor_companion_cop.pack` |

Uploading is done by the owner from their own Steam account. Nothing in this repository or in CI uploads anything.

## What the format is, and how it was found out

Sources: GSC's [Official Modding Guide](https://steamcommunity.com/sharedfiles/filedetails/?id=3497576322) and the
text strings of the three installed `xrEngine.exe` files (read only; the games were not started).

- **The uploaded folder holds nothing but `.pack` files (and loose `.mp4`).** The guide says so, and the engine
  agrees: next to its Workshop messages (`[SWS] Pack files not found!`, `[SWS] Registering pack '%s'`) it carries
  exactly two file masks, `.pack` and `.db`. There is no `.xrp` and no `desc.json` anywhere in the engine.
  The earlier packager wrote `<name>.xrp` plus `desc.json` into one folder — the game would have answered
  "Pack files not found". Fixed: the archive is `upload/<name>.pack`, alone in its folder.
- **Title, description, preview and change note are arguments of GSC's uploader**, not files in the package:
  `xrSWS_Upload.exe --mode=create --game=<SOC|CS|COP> --path=<folder> --preview=<picture> --title=… --desc=… --changelog=…`.
  `desc.json` stays as this editor's own record next to the upload folder; the game never sees it.
- **A package does not start a script by itself.** The archive header's `entry_point` is the mount point of the
  files (`$fs_root$\gamedata\`), not a script to run, and X-Ray has no script loader. The mod runs only when three
  of the game's own files are changed: `scripts/bind_stalker.script` (one line that calls the mod every frame),
  `scripts/ui_main_menu.script` (Esc, then F1 opens the menu) and `misc/quest_items.ltx` (includes the companion
  items). The earlier package carried none of them, so a subscriber would have got a mod that never runs.
  Fixed: `--hooks-from` reads those three files from the installed game, applies the same exact-line patches as
  the editor's own installer and puts them into the archive. They are GSC's files, so they are **not** in this
  repository and not in CI artifacts; they exist only in a package staged on a machine that has the game.
- **Shadow of Chornobyl reads configs from `gamedata/config`**, the other two from `gamedata/configs`
  (`fsgame_soc.ltx`, `fsgame_cs.ltx`, `fsgame_cop.ltx`). The earlier SoC package used `configs`. Fixed.

## 1. Stage the packages

On a machine with the three Enhanced Editions installed (the game folders are only read):

```sh
cargo run --release -p sse-ui --bin sse-ui-dev -- --package-ee soc out/stalker-soc-ee \
  --hooks-from "<Steam>/steamapps/common/STALKER Shadow of Chornobyl - EE"
cargo run --release -p sse-ui --bin sse-ui-dev -- --package-ee cs out/stalker-cs-ee \
  --hooks-from "<Steam>/steamapps/common/STALKER Clear Sky - EE"
cargo run --release -p sse-ui --bin sse-ui-dev -- --package-ee cop out/stalker-cop-ee \
  --hooks-from "<Steam>/steamapps/common/STALKER Call of Prypiat - EE"
```

Stage from a clean game: if the companion is already installed there, the three files are read with the hook
already in them (that is fine), but any other mod's changes to them would be carried into the package.

Each folder then holds:

| Path | What it is |
|---|---|
| `upload/save_editor_companion_<game>.pack` | The only thing that is uploaded |
| `preview.jpg`, `preview.png` | Preview picture, 1024×1024 (`tools/make_workshop_previews.py`) |
| `listing/title.txt` | Title of the Workshop item |
| `listing/short-description.txt` | One line for `--desc` |
| `listing/description.txt` | Full description, English then Russian, in Steam markup — paste it on the item page |
| `listing/changelog.txt` | Change note for 2.0.0, English and Russian |
| `listing/tags.txt` | Tags to tick on the item page |
| `upload-command.txt` | The upload command with this package's title and code |
| `desc.json` | The editor's own record (`starts_in_game` must be `true`) |

Without `--hooks-from` (this is what CI stages) the folder also has `NOT-READY.txt` and `"starts_in_game": false`:
the archive is complete as a container but nothing in the game calls the mod. **Do not upload such a package.**

## 2. Check in the game before uploading

There is no known way to load a `.pack` locally the way a subscription does: the game asks Steam where a
subscribed item is installed. So the check has two parts.

**The mod files, before any upload.** In each Enhanced Edition:

1. In the editor, on the Companion page of that game: install. This writes the very same files the package carries
   (the mod's scripts and configs plus the three patched game files) as loose files under `<game>/gamedata/`.
2. Start the game, load a save, press Esc, then F1. The companion menu must open (title in Russian, a tab rail
   on the left: Режимы, Игрок, Инвентарь, Выдача, NPC, Телепорт, Мир, Карта).
3. Try one action per tab — for example +10 000 RU on «Игрок» — and one command from the editor's Companion page
   while the game runs.
4. Remove it again with the uninstall button of the same page. The game folder must be back to what it was (`gamedata/` holds only
   what the game shipped). Do this before the next part, so that the loose files do not hide a broken package.

**The package itself, after the upload.** A new item is visible only to its author until GSC's moderators accept
it (the guide's "Testing Modification"). Subscribe to your own item, start the game and repeat steps 2–3.
To remove it: unsubscribe on the item page; Steam deletes
`<Steam>/steamapps/workshop/content/<app>/<item id>/`.

## 3. Upload

GSC's tools are not part of the game install. The guide links them: `xrSWS_Upload` is in `stk-utils.7z`.

1. Copy `xrSWS_Upload.exe` into the root folder of the Enhanced Edition you are uploading for (next to
   `xrEngine.exe`). Steam must be running and signed in to the account that will own the items.
2. Open a console in that folder and run the command from `upload-command.txt`, with both `<this folder>`
   placeholders replaced by the full path of the staged package, for example:

   ```bat
   xrSWS_Upload.exe --mode=create --game=COP --path=C:\ws\stalker-cop-ee\upload --preview=C:\ws\stalker-cop-ee\preview.jpg --title="Save Editor Companion — Call of Prypiat Enhanced Edition" --desc="…" --changelog="2.0.0"
   ```

   Running `xrSWS_Upload.exe` without arguments prints its help; check the flag names against it first.
3. The tool prints the new item's number. The item appears among your items on the game's Workshop page,
   hidden from everyone else until moderation accepts it.
4. On the item page: paste `listing/description.txt` as the description, `listing/changelog.txt` as the change
   note, tick the tags from `listing/tags.txt` that the page offers, and accept the Workshop agreement if Steam
   asks.
5. Repeat for the other two games, each from its own game folder with its own `--game` code.

## 4. Put the three numbers into the editor

In `crates/sse-companion/src/workshop.rs`, `EE_WORKSHOP_PACKAGES`, replace `published_file_id: None` with the
number of each item, as a string:

```rust
EnhancedEditionPackage {
    app_id: 2_427_430,
    published_file_id: Some("3600000000"),
    archive_name: "save_editor_companion_cop.pack",
    game: Game::CallOfPripyat,
},
```

The test `workshop_ids_stay_unpublished_until_the_owner_sets_them` in `crates/sse-companion/tests/workshop.rs`
guards the empty state; change it in the same commit.

What changes in the editor (the Companion page of an Enhanced Edition):

| State | Before | After |
|---|---|---|
| Line under the status | «Пакет Steam Workshop ещё не опубликован» | «Не подписан на пакет Steam Workshop», then «…установлен и актуален» or «…устарел или повреждён» |
| Workshop button | absent | opens `https://steamcommunity.com/sharedfiles/filedetails/?id=<number>` |

"Up to date" means: the subscribed `.pack` holds every mod file of this editor build, byte for byte. After a
later editor release changes the mod files, the state becomes "outdated" until the item is updated
(`xrSWS_Upload.exe --mode=update …` — see the tool's help).

## Not verified

- **Nothing here was run in a game.** No package was uploaded, no Enhanced Edition was started.
- **GSC's tools were not available**: `xrCompress` and `xrSWS_Upload` are not in the game installs and nothing was
  downloaded. The archive is written by this repository's own code following the open X-Ray archive layout; it was
  never compared with the output of GSC's `xrCompress -store`. The flag names of `xrSWS_Upload` come from the
  single example in the guide; `--mode=update` and how the tool reports the new item number are assumptions.
- **No Workshop item of any Enhanced Edition is subscribed on the build machine**, so the layout of a real
  downloaded item was not compared either. The `.pack`/`.db` masks come from the engine's strings, not from
  watching the engine load a mod.
- Whether the engine accepts our archive's header chunk (`[header] entry_point = $fs_root$\gamedata\`) for a
  Workshop pack, and whether a file from a pack overrides the same file from the game's own `resources` archives,
  is unknown. If it does not override, the three patched game files have no effect and the mod does not start.
- The guide's comments say the `xrCompress` build it links refuses the `configs` and `scripts` folders (it is the
  mod.io build). Our packager does not use it, but it shows that scripts in Workshop packs are less trodden
  ground than textures.
- Whether GSC's moderation accepts a mod that carries changed copies of three game scripts and adds cheats.
- Preview: the guide's example passes a `.jpg`; whether the uploader takes a `.png` was not checked. Both are
  staged.
- The tags the Enhanced Edition Workshop pages offer were not read; `listing/tags.txt` is a guess to pick from.
- Menu texts are Russian only; the description says so.
