# RisenLab

Asset pipeline tool for a fan-made AI-assisted remaster of **Risen 1** (Piranha Bytes, 2009).

This is not a level editor and not a new engine. The goal is a pipeline: unpack the game's
resources, convert them to open formats, run AI enhancement (texture upscale, normal/roughness
generation), review the result, and pack it back — for thousands of files, with a human only
approving/rejecting, not editing each asset by hand.

## Status

Early, but the full texture pipeline mechanics are proven end to end on a real game file:
unpack → transform → repack → verify all work. The transform step is still a placeholder
(Lanczos resize, not a real AI model — see `docs/ROADMAP.md`). Nothing beyond textures is
built yet.

## What's implemented (`src/`)

- **`pak.rs`** — Risen 1 `.pak`/`.pXX` container format: read (header, directory tree, zlib
  decompression) and write (build a fresh archive from a directory — for patch volumes).
  Verified byte-for-byte against real `library.pak` and `materials.pak` files (header
  `DataOffset`/`VolumeSize` match exactly; a full unpack → repack round trip reproduces
  identical file sizes and offsets).
- **`ximg.rs`** — Risen 1/2 `._ximg` texture format: extract the embedded standard DDS
  payload, read/patch the `Width`/`Height` property fields (`ximg-patch`). Verified end to
  end: a real 64x64 `._ximg` from the game was upscaled 4x and spliced back into a valid
  256x256 `._ximg` that re-parses and re-decodes correctly.
- **`tools/dxt3_encode.py`** — a small from-scratch DXT3/BC2 encoder (S3TC), used to produce
  a real compressed DDS for the round-trip test above without depending on an external
  image-compression library. Stands in for the eventual AI upscale step.

Run `cargo build --release` then `./target/release/risenlab --help` for the CLI
(`list`, `unpack`, `pack`, `ximg-to-dds`, `ximg-info`, `ximg-patch`).

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
