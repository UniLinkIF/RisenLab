# Distribution format: `.pXX` patch volumes (community mod-folder convention)

## ⚠️ This is no longer how `install_patches` installs into the owner's own game (2026-07-20/21)

The owner's own live test found the game did NOT pick up an `images.p01` sitting next to
`images.pak` — it kept showing the untouched original. Rather than keep guessing at the engine's
real override rule for this specific archive, `batch::install_patches` (see `src/batch.rs`) now
writes a FULL merged replacement `.pak` straight into the game's data folder — original bytes
verbatim except for the entries a patch actually changes — backing up the pristine original once
under `patch_dir/_originals/<group>/<stem>.pak` so `uninstall_patches` can restore it. This
sidesteps the `.pNN` uncertainty below entirely: the game already knows how to load an ordinary
complete archive, no override-rule guessing required.

`apply()` still builds `.pNN` volumes into `patch_dir` exactly as before — that's still the right
shape for the community mod-folder distribution described at the bottom of this file (sharing a
mod via the same tool ecosystem other Risen mods use). `install_patches` just no longer treats
`.pNN`-next-to-`.pak` as the install mechanism for the owner's own game; it reads those same
`.pNN` files as the source of the merge instead.

## Why `.pNN` existed as an idea in the first place

The final output of this pipeline shouldn't be a modified copy of the game's original `.pak`
files — redistributing those would mean redistributing Piranha Bytes' copyrighted assets.
Risen's own engine, in theory, already has a mechanism to avoid that (below) — but see the
warning above for why the owner's own install path doesn't rely on it anymore.

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

**Not confirmed, and empirically FAILED for `images.pak` specifically (2026-07-20)**: the owner
built a real `images.p01` (via `apply()`), installed it next to the real `images.pak`, launched
Risen, and the game showed the untouched original — not the approved AI texture. Whatever the
exact override rule is (does the highest-numbered suffix always win? is it strictly mount
order? does `data/compiled/images` need something beyond `mountlist_packed.ini`'s existing
`[Packed] ... data/compiled/images` entry to even scan for `.pNN` siblings?), it did not trigger
for this archive in this real, licensed install. The only supporting evidence for the mechanism
working at all is `Rimy3D`'s own texture-search logic (resolves conflicts by taking the first
match in a user-ordered list — the tool author modeling probable engine behavior for browsing
purposes, not proof), and a real Risen mod (World of Players' "Risen Unofficial Patch") that
successfully used `.pNN` for `templates`/`library`/`sounds`/`speech_english` — notably **never**
for `data/compiled/images`. It's possible `.pNN` genuinely works for some archive families and
not others; that's unconfirmed, not ruled out. **This is why `install_patches` no longer relies
on it** — see the warning at the top of this file.

**Still to do, if ever revisited**: check whether Risen writes a startup log with archive-load
errors, whether a `Project.prj`-style registry file lists mounted `.pNN`s explicitly, and whether
the ModDB "Risen 3 Resource Manager" tool's own docs say anything concrete about the override
rule. Low priority now that `install_patches` doesn't depend on the answer.

## Mod folder convention (community, from ModStarter's readme)

Not required by the engine, but this is what existing tools/users expect, and matching it means
our output can be installed with the same familiar tool instead of a bespoke installer:

- `Fanmods/<mod name>/{compiled,common}/*.p0x` — the patch volume(s).
- One `*.jpg` cover image (330x327 or 315x312).
- `readme.txt`/`readme.rtf` — short description shown in the mod manager's panel.
- `*Mod.csv` / `*Mod_<Language>.csv` — text/dialogue patch files, keyed by original table name.
- `*Mod.wrldatasc` — world/level data patch (ini-like: only new/changed sections+keys).
- `Modname.ini` — `[Name]\nCaption=...` to control the display name.
