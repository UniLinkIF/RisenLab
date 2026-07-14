# Distribution: `.pXX` patch volumes, not rewritten `.pak` files

## Why

The final output of this pipeline shouldn't be a modified copy of the game's original `.pak`
files — redistributing those would mean redistributing Piranha Bytes' copyrighted assets.
Risen's own engine already has a mechanism to avoid that.

## The mechanism

Risen's data folders (`Risen/data/compiled/`, `Risen/data/common/`) hold base archives like
`images.pak`, `materials.pak`, etc. The engine supports layering incremental patch volumes
next to them, using the *same container format* (see `pak.md`) under a numbered extension:

```
images.pak   <- base
images.p01   <- first patch/mod
images.p02   <- second patch/mod
```

Confirmed from two independent sources:

- `RisenPAK.exe`'s own usage note: if no destination is given, it appends `.00` — i.e. its
  native output convention for a freshly built volume is a numbered suffix, not `.pak`.
- The **Risen ModStarter** tool (`RMDS_OnlineDB_*.exe`, LordOfWAR/WorldOfRisen.de +
  Odin68/Mighty DWARF) auto-increments this exact suffix when installing a mod: "if
  `templates.p02` already exists, the new mod's volume becomes `templates.p03`". Its file-open
  dialog filter in `Rimy3D` also lists both conventions side by side:
  `"Genome Volumes and Patches (*.pak *.p00 *.00 *.p0? *.0?)"`.
- The `.deleted` marker convention in `RisenPAK.txt` (see `pak.md`) exists specifically "to aid
  creating patch volumes that have to delete files in an existing PAK volume" — i.e. patch
  volumes are expected to both add and remove entries relative to the base.

## What's confirmed vs. assumed

**Confirmed**: the naming convention, the fact that it's the same container format, and that
deletion markers exist for patch use.

**Not yet confirmed empirically**: the exact override rule the game engine applies when the
same filename exists in both a base volume and a patch (does the highest-numbered suffix always
win? is it strictly mount order?). The only supporting evidence so far is `Rimy3D`'s own
texture-search logic, which resolves conflicts by taking the first match in a user-ordered
list — that's the tool author modeling probable engine behavior for browsing purposes, not
proof of the engine's actual behavior.

**To do**: build a minimal test `.p01` with one deliberately conflicting file against a real
base `.pak`, load the game, and observe which version it uses. Needs a Windows machine with
Risen installed (tracked as a milestone, not yet done).

## Mod folder convention (community, from ModStarter's readme)

Not required by the engine, but this is what existing tools/users expect, and matching it means
our output can be installed with the same familiar tool instead of a bespoke installer:

- `Fanmods/<mod name>/{compiled,common}/*.p0x` — the patch volume(s).
- One `*.jpg` cover image (330x327 or 315x312).
- `readme.txt`/`readme.rtf` — short description shown in the mod manager's panel.
- `*Mod.csv` / `*Mod_<Language>.csv` — text/dialogue patch files, keyed by original table name.
- `*Mod.wrldatasc` — world/level data patch (ini-like: only new/changed sections+keys).
- `Modname.ini` — `[Name]\nCaption=...` to control the display name.
