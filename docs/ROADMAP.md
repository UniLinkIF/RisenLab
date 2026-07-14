# Roadmap

## Done

- [x] `.pak`/`.pXX` container: read + write, verified on real files (`library.pak`, `materials.pak`)
- [x] `._ximg` texture: read (extract DDS), understand write path (patch Width/Height + splice)
- [x] Confirmed `.pXX` patch-volume distribution model (no redistribution of original assets)
- [x] Decision on content-layer formats (materials/meshes): reuse `mimicry` out-of-process, don't reimplement

## Next

- [ ] Wire `ximg::replace_dds` into the CLI (`risenlab ximg-patch <in> <new.dds> <out>`)
- [ ] Test the zlib decompression path against a real compressed `.pak` entry (likely in `images.pak`)
- [ ] Empirically confirm `.pXX` override/priority rule against the real game (needs Windows + Risen install)
- [ ] First real AI step: wire an existing open upscaler (e.g. Real-ESRGAN) as an external process over an extracted DDS/PNG
- [ ] End-to-end proof: unpack one real texture from `images.pak` → upscale → repack into a `.p01` → load in-game

## Later

- [ ] Compression on write (`pak::write_archive_from_dir` currently always uncompressed)
- [ ] Subfolder support in the `.pak` writer (currently flat root only)
- [ ] Normal map / roughness generation step
- [ ] Orchestrator: batch job queue, caching, retry
- [ ] Asset versioning/approval store
- [ ] Review UI (Tauri + Svelte + three.js)
- [ ] Materials/meshes helper (compiled `mimicry`, called out-of-process)
- [ ] Model/animation pipeline: export/preview/import only, no AI enhancement (no mature tooling exists for this yet)
