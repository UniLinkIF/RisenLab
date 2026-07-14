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
      synthetic install layout built from real `library.pak`/`materials.pak`, and later against the real
      game install too (see the `.lnk` entry below).
- [x] **Generalized `ximg-patch`** to also cover `SkipMips` and `PixelFormat`, not just dimensions
      (`--skip-mips`, `--pixel-format`). A pixel format name of a different byte length now correctly
      shifts `property_block_size`/`dds_offset` — verified both on synthetic fixtures and on the real
      Forest texture (`DXT3` → `UNCOMPRESSED_RGBA`, +13 bytes, `dds_offset` moved from 265 to 278 exactly,
      DDS signature confirmed at the new offset).
- [x] **DDS ↔ plain PNG (`src/dds.rs`, `ximg-to-png`/`png-to-ximg`)**: decode/encode DXT1/DXT3/DXT5
      via `texpresso` (pure-Rust, MIT, from-scratch S3TC — not the GPL `mimicry` codec) plus
      uncompressed formats (any byte-aligned bit depth via generic channel-bitmask packing:
      A8R8G8B8, A8B8G8R8, L8, A8, ...). Width/height/pixel-format are all auto-detected from the
      original texture and the replacement PNG — no more manual `--width`/`--height` byte math.
      Verified on a real 1024x1024 normal map from the game (`Level/Nat_Stone_Rock_01_Normal_01`):
      decode→PNG→encode→re-decode is visually identical and `ximg-info` re-parses cleanly.
- [x] **Subfolder support in the `.pak` writer** (was flat-root-only): `write_archive_from_dir` now
      builds a real nested directory tree matching what `read_directory` expects. Verified against
      the real 590MB `images.pak` (1343 files, deep `Level/`/`Animation/`/etc. subfolders):
      unpack → repack → unpack again, all 1343 files byte-identical to the original.
- [x] **Zlib decompression verified against real compressed `.pak` entries**: `compiled/materials.pak`'s
      two real `ZLib`-compressed entries (`ShaderMaterialPool_Master.smp`, `compiled_materials.bin`)
      decompress to exactly their header-declared sizes.
- [x] **`resolve_shortcut` verified against a real Windows-generated `.lnk`** — and this found two
      real bugs, both fixed: (1) `LocalBasePathOffset` was read from the wrong struct offset (+12,
      actually `VolumeIDOffset`; spec says +16); (2) `LocalBasePath` is ANSI-codepage-encoded, not
      UTF-8 — decoding a Cyrillic path with `from_utf8_lossy` corrupted it. Now decoded via
      `MultiByteToWideChar(CP_ACP, ...)`. A real shortcut to `Risen.exe` through a Cyrillic folder
      name now resolves correctly end to end.
- [x] **Batch texture pipeline (`src/batch.rs`, `extract-textures`/`apply-textures`)**: point at the
      game once, every `._ximg` in every discovered archive is decoded to a mirrored tree of plain
      PNGs plus a manifest; `apply-textures` re-encodes only the PNGs that actually changed (content
      hash) and packs them into fresh, minimal `.pXX` patches, one per source archive. Run against
      the real, full game install: 1342/1342 textures extracted successfully (0 failures) after
      fixing two real-world pixel-format gaps found this way — `L8` (ddsfile doesn't populate
      channel bitmasks for `LUMINANCE`-only formats, only `RGB`) and `A8` (the uncompressed unpacker
      assumed every format was 4 bytes/pixel, breaking on this real 8bpp alpha-only blend mask).
- [x] **Full `extract-textures` → edit → `apply-textures` cycle proven on the real game**: extracted
      all 1342 textures, inverted two PNGs (an achievement icon + the 1024x1024 normal map) in an
      ordinary image editor stand-in, ran `apply-textures` — it correctly built a minimal 2-entry
      `compiled/images.p01` (not all 1342), and the patched entries re-parse and re-decode with the
      edit visibly applied. Test patch built at
      `%TEMP%/risen_patch_out2/compiled/images.p01` — not yet copied into the real game install
      (needs the user to do it, or explicit sign-off to modify that folder; see below).
- [x] **Full mip chain on encode** (`dds::build_mip_chain`/`downsample_half`): `png-to-ximg`/
      `apply-textures` now write a proper mip chain (box-filter downsample, level 0 down to 1x1)
      instead of a single level. Verified on the real 1024x1024 normal map: re-encoded file size
      matches the original's real mip-chain size almost exactly, and still decodes visually
      identical (decode always reads only the top level regardless of chain length).
- [x] **Review HTML** (`risenlab review-textures`): a single self-contained HTML page (images
      inlined as base64, no external files) showing original-vs-edited side by side for every
      texture that changed since extraction — a cheap stand-in for a dedicated review UI, open it
      in any browser before running `apply-textures`.

## Next

- [ ] Replace the Lanczos placeholder in the pipeline with a real AI upscaler as an external process —
      the splice/patch mechanics around it already work, this is meant to be a drop-in swap. First attempt
      (auto-downloading a pretrained EDSR model via `super_image`/HuggingFace) was correctly blocked by
      the session's security policy — autonomously fetching+running third-party model weights needs
      explicit user sign-off on the specific model/source, not an agent decision. Needs that decision, or
      to happen on the user's own machine under their control.
- [ ] Empirically confirm `.pXX` override/priority rule against the real game, and visually confirm the
      inverted-color test patch actually shows up in-game — now technically unblocked (Windows + real
      Risen install both present, test patch already built), but copying the patch into the real game's
      `data/compiled/` folder was declined by the session's safety policy (modifying a folder the user
      pointed at as reference material, not something they explicitly authorized changing) and needs
      either the user to copy the file themselves or explicit sign-off to do it directly.
- [ ] Materials (`.xmat`) / meshes (`.xmsh`/`.xmac`) / animations (`.xact`) support — scope expanded to
      include these (previously texture-only). Researched `mimicry` (github.com/Baltram/rmtools,
      GPL-3.0) as the out-of-process helper: it's pure portable C++ (no Qt/MFC dependency, builds
      cleanly), and already has a clean API for everything needed — `mCGenomeMaterial::Load/Save` +
      generic `GetProperty`/`SetProperty` for materials (no need to hand-derive the ~230-class property
      table, it's already inside), `mCXmshReader`/`mCXmshWriter` + `mCObjReader`/`mCObjWriter` for
      mesh↔OBJ (Blender-editable), `mCXactReader` for animations — all built around a common `mCScene`
      representation. Vendoring the GPL source and building the actual helper binary was declined by
      the session's safety policy (the scope-expansion instruction arrived over an external/untrusted
      channel, not this session directly) — needs the user to confirm directly in a trusted session
      before that specific step proceeds. Everything else about the plan above is solid and ready to
      execute once confirmed.
- [ ] Handle the ~4 remaining known-exotic DDS pixel formats we haven't hit yet in real assets
      (DX10/DXGI header, ATI1/ATI2, YUV) — everything encountered in the real game so far decodes.

## Later

- [ ] Compression on write (`pak::write_archive_from_dir` currently always uncompressed)
- [ ] Orchestrator: batch job queue, caching, retry
- [ ] Asset versioning/approval store
- [ ] Review UI beyond the static HTML page (Tauri + Svelte + three.js)
