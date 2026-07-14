# Roadmap

## Done

- [x] `.pak`/`.pXX` container: read + write, verified on real files (`library.pak`, `materials.pak`)
- [x] `._ximg` texture: read (extract DDS), understand write path (patch Width/Height + splice)
- [x] Confirmed `.pXX` patch-volume distribution model (no redistribution of original assets)
- [x] Decision on content-layer formats (materials/meshes): reuse `mimicry` out-of-process, don't reimplement
- [x] Wire `ximg::replace_dds` into the CLI (`risenlab ximg-patch <in> <new_dds> <out> --width W --height H`)
- [x] **Full texture pipeline round trip proven on a real game file**: `EditorBilboard_EVT_Sound_Forest._ximg`
      (64x64) → extract DDS → 4x Lanczos upscale → re-encode as genuine DXT3 (`tools/dxt3_encode.py`, a
      from-scratch S3TC/BC2 encoder — no external image-compression library needed) → `ximg-patch` splices
      it back in → re-parsed and re-decoded successfully as a valid 256x256 image. Proves the mechanical
      pipeline (unpack → transform → repack → verify) works; the transform step itself is still a placeholder
      (Lanczos, not a real AI model) — see next item.
- [x] **Game auto-discovery** (`src/gamepath.rs`, `risenlab discover <exe-or-.lnk>`): point at `Risen.exe`
      (or a `.lnk` shortcut to it — minimal MS-SHLLINK `LinkInfo`/`LocalBasePath` parser, no PowerShell
      dependency) and the tool walks up to the game root, then recursively finds every archive under
      `data/` (`.pak` and `.pXX`/`.0X` patch volumes), grouped by `compiled`/`common`. Verified against a
      synthetic install layout built from real `library.pak`/`materials.pak`. The `.lnk` parser hasn't been
      exercised against a real Windows-generated shortcut yet — untested edge case, see below.
- [x] **Generalized `ximg-patch`** to also cover `SkipMips` and `PixelFormat`, not just dimensions
      (`--skip-mips`, `--pixel-format`). A pixel format name of a different byte length now correctly
      shifts `property_block_size`/`dds_offset` — verified both on synthetic fixtures and on the real
      Forest texture (`DXT3` → `UNCOMPRESSED_RGBA`, +13 bytes, `dds_offset` moved from 265 to 278 exactly,
      DDS signature confirmed at the new offset).

## Next

- [ ] Replace the Lanczos placeholder in the pipeline with a real AI upscaler as an external process —
      the splice/patch mechanics around it already work, this is meant to be a drop-in swap. First attempt
      (auto-downloading a pretrained EDSR model via `super_image`/HuggingFace) was correctly blocked by
      the session's security policy — autonomously fetching+running third-party model weights needs
      explicit user sign-off on the specific model/source, not an agent decision. Needs that decision, or
      to happen on the user's own machine under their control.
- [ ] Test the zlib decompression path against a real compressed `.pak` entry (likely in `images.pak`)
- [ ] Empirically confirm `.pXX` override/priority rule against the real game (needs Windows + Risen install)
- [ ] End-to-end proof: unpack one real texture from `images.pak` → upscale → repack into a `.p01` → load in-game
- [ ] Verify `resolve_shortcut` against a real Windows-created `.lnk` to Risen.exe (Desktop/Start Menu
      shortcuts can carry LinkTargetIDList-only shortcuts our parser doesn't handle yet — falls back to
      an error rather than silently guessing)

## Later

- [ ] Compression on write (`pak::write_archive_from_dir` currently always uncompressed)
- [ ] Subfolder support in the `.pak` writer (currently flat root only)
- [ ] Normal map / roughness generation step
- [ ] Orchestrator: batch job queue, caching, retry
- [ ] Asset versioning/approval store
- [ ] Review UI (Tauri + Svelte + three.js)
- [ ] Materials/meshes helper (compiled `mimicry`, called out-of-process)
- [ ] Model/animation pipeline: export/preview/import only, no AI enhancement (no mature tooling exists for this yet)
