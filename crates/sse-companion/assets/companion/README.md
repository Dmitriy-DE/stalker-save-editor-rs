# Save Editor companion (in-game)

A small script mod. While the game runs, the editor sends a command file and
the mod carries it out in the game within about 2 s: give items, money, heal,
repair worn gear, teleport inside the level, list inventory. No save reload.

Protocol: [docs/COMPANION.md](../../docs/COMPANION.md).

## Games

| Game | Works | Notes |
|---|---|---|
| Shadow of Chernobyl 1.0004/1.0006 | all commands | `io`/`os` confirmed only via the OGSR fork source |
| Clear Sky 1.5.10 | all commands | |
| Call of Pripyat 1.6.02 | all commands | |
| SoC / CS / CoP Enhanced Edition | all commands | packaging for Workshop — see below |

Every engine function used is present in each game's vanilla
`lua_help.script` export dump (links in the script header). Nothing was run in
a game yet: the owner checks each command in each game before the editor
offers the companion to users.

## Install (original games)

1. Copy `gamedata/scripts/save_editor_companion.script` into
   `<game>/gamedata/scripts/`.
2. Vanilla X-Ray has no mod loader, so `bind_stalker.script` needs one line
   right after `object_binder.update(self, delta)` in
   `actor_binder:update(delta)` (line 215 SoC, 253 CS, 246 CoP in vanilla):

   ```lua
   if save_editor_companion then save_editor_companion.update() end
   ```

   If `gamedata/scripts/bind_stalker.script` is not there, the game reads the
   vanilla one from its `.db` archives; extract it first. The editor will do
   this step itself (install/uninstall with a backup of the file) — the file is
   not shipped here because it is GSC's.
3. `fsgame.ltx` must read `gamedata` (`$game_data$ = true| true| ...`).

Uninstall: remove the script and the line (or restore the backup).

## Enhanced Edition

EE loads mods through Steam Workshop / mod.io as `.xrp` packages built with
GSC's `xrCompress` and uploaded with `xrSWS_Upload` (see the WS1 research).
Packaging is a later step (plan Ф6); the script itself is the same.
