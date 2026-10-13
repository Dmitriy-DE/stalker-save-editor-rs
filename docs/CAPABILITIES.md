# What the editor can write, game by game

This is the full table behind the short one in the [README](../README.md#what-you-can-change-game-by-game). Players:
the short table and the [guide](GUIDE.md) are enough. This page is for those who want the reason behind every cell.

## Statuses

| Status | In the program | What it means |
|---|---|---|
| **Verified** | **Write** | The reference fixtures confirm the field: the editor's output matches them byte for byte. It does **not** mean a game has loaded the result. |
| **Experimental** | **Experim.** | The edit is allowed, checked by the writer and by a read-back, and measured on real saves, but no game has loaded it. |
| **Unsupported** | **No** | The editor refuses the edit. The reason is in the table. |

One edited Call of Pripyat save has been loaded in a game. Nothing else on this page has; see "Not verified" in the
[2.0.0 release notes](RELEASE_NOTES_2.0.0.md).

## X-Ray games

Taken from the capability registry in `crates/sse-xray/src/writer.rs` (`capability()`), which mirrors the C# 1.3.1
reference. The Capabilities screen reads the same function, so the screen and this table cannot disagree for these
six columns. ТЧ, ЧН and ЗП are the Russian abbreviations of Shadow of Chernobyl, Clear Sky and Call of Pripyat;
"12/12" means twelve of twelve real saves passed the edit, the read-back and the undo.

| Change | Shadow of Chernobyl, Clear Sky, Call of Pripyat | Enhanced Editions |
|---|---|---|
| Money | Verified | Experimental (measured on every real save: ТЧ EE 6/6, ЧН EE 8/8, ЗП EE 12/12) |
| Ammunition stacks | Verified | Experimental (ТЧ EE 4/4 and ЧН EE 6/6 measurable saves pass; ЗП EE 12/12; the other saves have no stack to edit) |
| Add items | Verified | Experimental (ТЧ EE 4/4, ЧН EE 6/6 measurable saves pass, undo byte-identical; ЗП EE 12/12; the other saves have no single-item template with a placement) |
| Remove items | Verified | Experimental (ТЧ EE 4/4 and ЧН EE 6/6 measurable saves pass; ЗП EE 12/12; the other saves have no backpack item to remove) |
| Equipment condition | Experimental | Experimental (not loaded in the game yet): ТЧ EE 6/6, ЧН EE 8/8, ЗП EE 12/12 (the restore keeps the client copy untouched where it differs from the state value) |
| Item placement | Experimental | Call of Pripyat EE: Experimental (not loaded in the game yet, 12/12); Shadow of Chernobyl EE, Clear Sky EE: Unsupported (slot occupancy cannot be proven: the equipped item reads as slot 0, so a slot move is refused) |
| Actor faction | Experimental | Experimental (26/26 real saves read the new faction and undo byte-identically) |
| Faction relations | Experimental | Unsupported (none of the 26 Enhanced Edition saves holds an actor relation entry, so nothing to edit) |
| Move items to or from a stash | Shadow of Chernobyl, Call of Pripyat: Experimental (the take into the backpack works; the return to a stash is refused until the game records the item's place). Clear Sky: Unsupported (a stash item has no proven placement field; the write refuses on 62 of 62 saves) | Unsupported (the move into the backpack succeeds, but its undo is not proven) |
| Info portions | Experimental | Unsupported |
| Relocate actor | Experimental | Unsupported |
| Upgrades | Clear Sky, Call of Pripyat: Experimental; Shadow of Chernobyl: Unsupported (not in the game) | Clear Sky EE, Call of Pripyat EE: Experimental (ЧН EE 8/8, ЗП EE 12/12 add and undo byte-identically); Shadow of Chernobyl EE: Unsupported (no upgrade catalog for it and no upgradeable item in the saves measured) |

**Actor faction has no control in the interface.** The writer supports it (`Change::SetPlayerFaction`) and the
Capabilities screen lists it, but no screen issues that change in this version. The Factions screen edits relations
only.

## S.T.A.L.K.E.R. 2

The S.T.A.L.K.E.R. 2 writer is `crates/sse-s2`. Its column on the Capabilities screen is written by hand in
`crates/sse-ui/src/screens/app.rs` (`capability_rows()`).

| Change | Status |
|---|---|
| Money | Experimental |
| Ammunition stack count | Experimental |
| Equipment condition | Experimental |
| Stash → backpack, backpack → stash | Experimental. The Stashes screen offers both directions (`MoveStashToBackpack`, `MoveBackpackToStash`). The Capabilities screen still shows **No** in this cell. |
| Item placement | Unsupported |
| Upgrades | Unsupported (the structure was not found in the save; there is no catalogue of names) |
| Faction relations, player faction | Unsupported (the numeric relations table was not isolated) |
| Add items | Unsupported (the length of an item record in the save is not confirmed) |
| Remove items | Unsupported |

Saves written by game version 1.0.x can be read but not edited: the layout of that version is not confirmed. Load
and save them once in the current game first.

## Measured on real saves

The final sweep before 2.0 ran on copies of 269 X-Ray saves and 33 S.T.A.L.K.E.R. 2 slots; no case of "written, but
reads back as something else" was found. Passed / applicable:

| Game | Money | Ammo | Add | Remove | Condition | Faction | Relations | From a stash |
|---|---|---|---|---|---|---|---|---|
| Shadow of Chernobyl (6 saves) | 6/6 | 5/5 | 5/5 | 5/5 | 5/5 | 6/6 | 0/5 in the sweep; 5/5 after a later fix (#575), measured on the save image | write 6/6, no undo |
| Clear Sky (62) | 62/62 | 59/59 | 62/62 | 62/62 | 62/62 | 59/59 | 59/59 | 0/62, switched off |
| Call of Pripyat (175) | 175/175 | 175/175 | 175/175 | 175/175 | 175/175 | 175/175 | 146/146 (29 have no relations) | write 175/175, no undo |
| Shadow of Chernobyl EE (6) | 6/6 | 4/4 | 4/4 | 4/4 | 6/6 | 6/6 | unsupported | switched off |
| Clear Sky EE (8) | 8/8 | 6/6 | 6/6 | 6/6 | 8/8 | 8/8 | unsupported | switched off |
| Call of Pripyat EE (12) | 12/12 | 12/12 | 12/12 | 12/12 | 12/12 | 12/12 | unsupported | switched off |
| S.T.A.L.K.E.R. 2 (33 slots) | 18 pass, 15 refused (1.0.x) | 18 pass, 15 refused (1.0.x) | | | | | | |

Item placement in Call of Pripyat: 174 of 175 (one refusal by the writer). Source: `ACCEPTANCE.md`, part V,
section 12.

A pass here means the editor read back what it wrote and undid it to the original bytes. It is not a game run.
