# RisenLab

Asset pipeline tool for a fan-made AI-assisted remaster of **Risen 1** (Piranha Bytes, 2009).

This is not a level editor and not a new engine. The goal is a pipeline: unpack the game's
resources, convert them to open formats, run AI enhancement (texture upscale, normal/roughness
generation), review the result, and pack it back — for thousands of files, with a human only
approving/rejecting, not editing each asset by hand.

## Status

The full texture pipeline works end to end against the real, whole game install: point at
`Risen.exe`, every texture in every archive extracts to a plain PNG (`extract-textures`),
edit/regenerate whichever ones you want, and `apply-textures` builds a minimal `.pXX` patch
containing only what changed. Verified at scale — 1342/1342 real textures extracted with 0
failures, a real edit-and-patch cycle produced a correct 2-entry patch, and a 590MB real
archive round-trips byte-identical through the `.pak` writer. The AI step itself is still a
placeholder (Lanczos resize, not a real model — see `docs/ROADMAP.md`); nothing beyond
textures (materials/meshes/animations) is built yet, and there's no review UI — you point an
ordinary image editor at the extracted PNG folder.

## What's implemented (`src/`)

- **`pak.rs`** — Risen 1 `.pak`/`.pXX` container format: read (header, directory tree, zlib
  decompression) and write (build a fresh archive from a directory — for patch volumes).
  Verified byte-for-byte against real `library.pak` and `materials.pak` files (header
  `DataOffset`/`VolumeSize` match exactly; a full unpack → repack round trip reproduces
  identical file sizes and offsets).
- **`ximg.rs`** — Risen 1/2 `._ximg` texture format: extract the embedded standard DDS
  payload, read/patch the `Width`/`Height`/`SkipMips`/`PixelFormat` property fields
  (`ximg-patch`).
- **`dds.rs`** — decodes/encodes the DDS payload itself to plain RGBA8, so a texture round-
  trips through an ordinary PNG instead of requiring DDS-aware tools: DXT1/3/5 via
  `texpresso` (pure-Rust, MIT, from-scratch S3TC — not the GPL `mimicry` codec), plus
  uncompressed formats at any byte-aligned bit depth (`A8R8G8B8`, `A8B8G8R8`, `L8`, `A8`, ...)
  via generic channel-bitmask packing. Verified against real textures from the game,
  including a 1024x1024 normal map and two 8bpp single-channel formats that needed real
  bug fixes to `ddsfile`'s format detection to support.
- **`batch.rs`** — the whole-game conveyor: `extract-textures` walks every discovered
  archive and decodes every `._ximg` to a mirrored tree of PNGs plus a manifest;
  `apply-textures` re-encodes only the PNGs that changed (by content hash) and packs them
  into fresh `.pXX` patches, one per source archive. Run against the real game: 1342/1342
  textures extracted, and a real edit-two-PNGs-then-patch cycle produced a correct, minimal
  2-entry patch.
- **`gamepath.rs`** — "point at `Risen.exe` (or a `.lnk` shortcut to it), we take it from
  there": resolves a Windows shortcut to its target (own minimal `.lnk` parser, no PowerShell
  needed — including the ANSI-codepage decoding real non-ASCII install paths need), walks up
  from the exe to find the game root, then recursively finds every archive under `data/`.
  This is the *only* manual step in the app — everything downstream (which `.pak`s exist,
  what's in them) is discovered automatically. Verified against a real Windows-generated
  `.lnk` to a real install.
- **`tools/dxt3_encode.py`** — a small from-scratch DXT3/BC2 encoder (S3TC), used for an
  early proof-of-concept round trip before `dds.rs` existed; superseded by it for normal use.

Run `cargo build --release` then `./target/release/risenlab --help` for the CLI (`list`,
`unpack`, `pack`, `ximg-to-dds`, `ximg-info`, `ximg-patch`, `ximg-to-png`, `png-to-ximg`,
`discover`, `extract-textures`, `apply-textures`).

## Why these formats and not others

Risen 1 runs on Piranha Bytes' **Genome engine**. Two format layers exist for almost every
resource type:

1. **Container layer** (`.pak`) — fully documented by Nico Bendlin (`RisenPAK.txt`, 2009-2011,
   MIT-style license), independently re-implemented here.
2. **Content layer** (what's *inside* a file entry — `.xmat` materials, `.xmsh`/`.xmac` meshes,
   `.ximg` textures) — a generic, RTTI-like property/reflection serialization used across the
   engine. For most resource types (materials, meshes) this requires a large hand-built table
   of class/property descriptors that only exists in `mimicry` (GPL-3.0, from the `rmtools`
   project) — not worth independently reverse-engineering.

**Textures are the exception**: `._ximg` wraps a small, fixed-size property block around a
completely standard, off-the-shelf DDS file. That's simple enough to implement cleanly here,
with no GPL dependency — see `docs/formats/ximg.md`.

Given this, textures are the highest-priority, most tractable, and highest-value module (AI
upscale is also the most mature AI tooling available), so they're the current focus. Materials/
meshes will most likely be handled by shelling out to a compiled `mimicry`-based helper rather
than reimplementing their format independently — see `docs/formats/content-layer.md`.

## Distribution model

Risen's engine natively layers patch volumes on top of a base archive
(`images.pak` + `images.p01` + `images.p02` + ...). This means the final output of this tool
should be a small `.pXX` patch, not a rewritten copy of the original `.pak` — see
`docs/p0x-patches.md`. This avoids redistributing any of the original game's copyrighted assets.

## Planned architecture (not yet built)

- **Core I/O** (this crate) — format read/write, no UI.
- **AI adapters** — thin wrappers around existing open tools (Real-ESRGAN-style upscalers,
  DeepBump-style normal/roughness generators), not custom models.
- **Orchestrator** — batch job queue, caching, retry.
- **Asset store / versioning** — content-addressed history per asset (original → AI v1 → v2 →
  approved), so any version can be restored.
- **Review UI** — Tauri + Rust core + a web-based front end (Svelte/React) + three.js for
  model/texture preview. Chosen for near-instant startup (no Electron/Chromium overhead) and
  full control over a clean, minimal UI.

## Licensing note

`pak.rs` and `ximg.rs` are original implementations written from published/independently
verified format specifications, not derived from GPL code. If a future module wraps or calls
GPL-3.0 `mimicry` code for materials/meshes, keep it as a separate out-of-process helper so it
doesn't require the rest of this project to be GPL.
