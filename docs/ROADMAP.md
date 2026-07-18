# Roadmap

This file tracks intent and open work, not a changelog — see `git log` for what actually
shipped and when. Rewritten 2026-07-18: the previous version stopped tracking around the
"materials/meshes/animations declined pending confirmation" phase, which is long resolved —
that whole layer is built and in daily use.

## Done

Engine/format layer (`src/`), all verified against the real game, not synthetic data only:
- `.pak`/`.pXX` container: read + write, incl. subfolders and ZLib-compressed entries.
- `._ximg` texture: full read/write round trip (DDS extract/splice, Width/Height/PixelFormat/
  SkipMips patching, full mip-chain encode). DXT1/3/5 + uncompressed formats (L8/A8/etc.).
- Game auto-discovery from `Risen.exe` or a `.lnk` shortcut, including real Cyrillic-path
  `.lnk` parsing.
- Meshes (`._xmsh`) and actors/skeletons (`._xmac`) via `mimicry-helper` (sibling GPL-3.0
  helper, out-of-process) for OBJ export, plus a from-scratch Rust skinned-mesh parser
  (`xmesh_skin.rs`) for real per-vertex bone weights (CPU skinning, not three.js's
  `SkinnedMesh` — see `SkeletonAnimationViewer.tsx` for why).
- Motion clips (`._xmot`, `xmot.rs`): real keyframe parsing incl. the header-key-count fix
  that untangled rotation vs. scale-rotation channels, in-place value/time patching
  (`patch_motion_keys`), jitter smoothing, and four animation-quality transforms
  (`stylize_tracks`: expressiveness / secondary motion / attack retiming / preview-only
  60fps resampling — see "Known gaps" below for what's NOT wired to export).
- AI texture enhancement (`ai.rs`): Replicate (default `real-esrgan`, any img2img model
  opt-in) + Stability AI conservative upscale, both via `curl.exe` (no Rust TLS stack builds
  in the dev sandbox). Auth header goes through a short-lived curl config file, never argv.

App (`app/`, Tauri + React + three.js):
- Three screens — Library (batch texture browsing/enhancement), Models (per-mesh
  texture/material generation, 3D preview), Animations (per-clip quality transforms, A/B and
  side-by-side compare, patch export) — plus Settings and a shared review queue (AiCompare).
- Review queue never auto-navigates the user away from what they're doing (Library/Models
  batch or single regenerate both just update a persistent Titlebar badge); the badge is the
  one deliberate way in.
- Packaged beta: real app icon, GitHub Actions (`build-windows.yml`, manual dispatch) builds
  on a real `windows-latest` runner and uploads NSIS/MSI installers + a portable exe — the dev
  sandbox itself can't link a Tauri binary (no MinGW `dlltool`), so this is the actual release
  path now, not a local `cargo build`.

## Known gaps (as of 2026-07-18 audit)

- **60fps is preview-only.** `resample_double_rate` changes key counts, and `patch_motion_keys`
  can only patch VALUES in place at a FIXED count — turning this into a real exportable patch
  needs the `.xmot` chunk-size wrapper fields decoded (not done) so a resized record's
  surrounding structure stays valid.
- **Specular→roughness mapping is a best-effort heuristic, not a verified one.** Both 3D
  viewers now derive a `_Specular_` texture name (same convention as `_Normal_`) and convert
  its luminance to a roughness map (`lib/roughness.ts`) instead of a flat hardcoded value —
  but Genome's exact specular-channel semantics (plain intensity vs. tinted color vs. a packed
  gloss value) aren't confirmed from the file format the way the DXT5nm normal-map swizzle
  was. Needs an owner eyeball-check against the real game.
- **In-game animation-patch verification is still open.** The texture-patch install path has
  been run at least once; there's no equivalent confirmation yet that the animation-quality
  transforms/patches look right running in real Risen, only in the app's own viewers.
- **`obj-to-mesh` in `mimicry-helper`** (the import direction — OBJ back into the game's mesh
  format) writes a file that doesn't re-parse. Not root-caused. `mesh-to-obj`/`material-dump`
  (export direction) are solid.
- **Titan "~10% black patches during animation"** — open, blocked on the owner reproducing it
  with a screenshot; every numeric diagnostic (materials/normals/weights/UV/unswizzle) has come
  back clean so far, so whatever's left is likely a render-state issue, not a data one.
- `.pXX` override/priority rule against the real game was never empirically confirmed
  end-to-end (texture patches have since been built/installed in later sessions, which
  presumably exercises this — not explicitly re-verified against this specific claim).

## Later / not started

- Compression on write for freshly-built `.pak`s (`write_archive_from_dir` writes uncompressed;
  patch volumes specifically DO support ZLib per-entry and that path is used).
- Batch job orchestration beyond what Library's sequential batch-enhance loop already does
  (no queue/retry/priority system).
- The ~4 remaining exotic DDS pixel formats never hit in real assets so far (DX10/DXGI header,
  ATI1/ATI2, YUV) — everything encountered in the real game to date decodes correctly.
