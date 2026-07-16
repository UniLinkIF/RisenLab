//! Batch conveyor for the texture round trip.
//!
//! `extract_all`: point at the game (exe or `.lnk`), and every `._ximg` texture in every
//! discovered archive is decoded to a plain PNG, mirroring the archive's internal folder
//! structure under `<group>/<archive-stem>/...`, alongside a manifest recording where each
//! PNG came from.
//!
//! `apply`: given that manifest and a directory of (possibly edited/AI-regenerated) PNGs,
//! only the ones that actually changed (by content hash) get re-encoded back into
//! `._ximg` and packed into fresh, minimal `.pXX` patch volumes — one per source archive,
//! containing only the changed entries, so a mod stays small and the original archive is
//! never touched (see `docs/p0x-patches.md`).

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;

use crate::{content, dds, gamepath, pak, ximg, xmac, xmesh_skin, xmot};

const MANIFEST_NAME: &str = "manifest.tsv";

/// Non-cryptographic FNV-1a hash, used only to detect "did the user change this PNG" —
/// no security property needed, so no extra dependency for it.
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

struct ManifestEntry {
    archive: PathBuf,
    group: String,
    entry_path: String,
    png_rel: String,
    hash: u64,
}

fn write_manifest_line(w: &mut impl Write, e: &ManifestEntry) -> Result<()> {
    writeln!(
        w,
        "{}\t{}\t{}\t{}\t{:016x}",
        e.archive.display(),
        e.group,
        e.entry_path,
        e.png_rel,
        e.hash
    )?;
    Ok(())
}

fn parse_manifest(path: &Path) -> Result<Vec<ManifestEntry>> {
    let text = fs::read_to_string(path).with_context(|| format!("reading manifest {}", path.display()))?;
    let mut out = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        let [archive, group, entry_path, png_rel, hash] = parts.as_slice() else {
            bail!("manifest {}: malformed line {}", path.display(), line_no + 1);
        };
        out.push(ManifestEntry {
            archive: PathBuf::from(archive),
            group: (*group).to_string(),
            entry_path: (*entry_path).to_string(),
            png_rel: (*png_rel).to_string(),
            hash: u64::from_str_radix(hash, 16)
                .with_context(|| format!("manifest {}: bad hash on line {}", path.display(), line_no + 1))?,
        });
    }
    Ok(out)
}

/// Discovers every archive reachable from `exe_or_shortcut`, extracts every `._ximg` entry as
/// a PNG into `out_dir`, and writes `manifest.tsv` there. Returns the number of textures
/// extracted. Entries that fail to decode (unsupported pixel format, corrupt data) are
/// skipped with a warning printed to stderr rather than aborting the whole run.
pub fn extract_all(exe_or_shortcut: &Path, out_dir: &Path) -> Result<usize> {
    let exe = gamepath::resolve_shortcut(exe_or_shortcut)?;
    let root = gamepath::discover_game_root(&exe).ok_or_else(|| {
        anyhow!("could not find a data/ folder with archives above {}", exe.display())
    })?;
    let archives = gamepath::discover_archives(&root)?;

    fs::create_dir_all(out_dir)?;
    let mut manifest = fs::File::create(out_dir.join(MANIFEST_NAME))?;

    let mut count = 0usize;
    for archive_info in &archives {
        let mut archive = match pak::PakArchive::open(&archive_info.path) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("skipping archive {}: {e}", archive_info.path.display());
                continue;
            }
        };
        let archive_stem = archive_info
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("archive")
            .to_string();

        for entry in &archive.files() {
            if entry.is_deleted() || !entry.path.to_ascii_lowercase().ends_with("._ximg") {
                continue;
            }
            let data = match archive.read_file(entry) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("skipping {}: {e}", entry.path);
                    continue;
                }
            };
            let decoded = ximg::extract_dds(&data)
                .map_err(anyhow::Error::from)
                .and_then(|dds_bytes| dds::decode(dds_bytes));
            let decoded = match decoded {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("skipping {} (unsupported/invalid): {e}", entry.path);
                    continue;
                }
            };

            let rel_inside = entry.path.trim_start_matches('/');
            let png_rel = Path::new(&archive_info.group)
                .join(&archive_stem)
                .join(rel_inside)
                .with_extension("png");
            let png_full = out_dir.join(&png_rel);
            if let Some(parent) = png_full.parent() {
                fs::create_dir_all(parent)?;
            }
            let img = image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba)
                .ok_or_else(|| anyhow!("decoded RGBA size mismatch for {}", entry.path))?;
            img.save(&png_full).with_context(|| format!("writing {}", png_full.display()))?;

            let png_bytes = fs::read(&png_full)?;
            let png_rel_str = png_rel.to_string_lossy().replace('\\', "/");
            write_manifest_line(
                &mut manifest,
                &ManifestEntry {
                    archive: archive_info.path.clone(),
                    group: archive_info.group.clone(),
                    entry_path: entry.path.clone(),
                    png_rel: png_rel_str,
                    hash: fnv1a(&png_bytes),
                },
            )?;
            count += 1;
        }
    }
    Ok(count)
}

/// Picks the next free `<stem>.pNN` suffix for `archive_path`, checking both the game's own
/// install folder (so we never collide with an already-installed mod) and `patch_out_dir`.
fn next_patch_path(archive_path: &Path, patch_out_dir: &Path, group: &str) -> Result<PathBuf> {
    let stem = archive_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("archive path has no file stem: {}", archive_path.display()))?;
    let game_dir = archive_path
        .parent()
        .ok_or_else(|| anyhow!("archive path has no parent: {}", archive_path.display()))?;
    let group_out_dir = patch_out_dir.join(group);

    for n in 1..100u32 {
        let name = format!("{stem}.p{n:02}");
        if !game_dir.join(&name).exists() && !group_out_dir.join(&name).exists() {
            return Ok(group_out_dir.join(name));
        }
    }
    bail!("no free .pXX patch slot for {stem} (checked p01..p99)")
}

/// Re-encodes every PNG in `edited_dir` whose content differs from the manifest's recorded
/// hash, and packs the changed entries into fresh `.pXX` patch volumes under `patch_out_dir`
/// (one per source archive, grouped by `<group>/<archive>.pNN`). Returns the patch files
/// written.
pub fn apply(manifest_path: &Path, edited_dir: &Path, patch_out_dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = parse_manifest(manifest_path)?;

    let mut by_archive: HashMap<PathBuf, (String, Vec<&ManifestEntry>)> = HashMap::new();
    for e in &entries {
        let png_full = edited_dir.join(&e.png_rel);
        if !png_full.exists() {
            continue;
        }
        let bytes = fs::read(&png_full)?;
        if fnv1a(&bytes) == e.hash {
            continue; // unchanged since extraction, nothing to patch
        }
        by_archive
            .entry(e.archive.clone())
            .or_insert_with(|| (e.group.clone(), Vec::new()))
            .1
            .push(e);
    }

    if by_archive.is_empty() {
        return Ok(Vec::new());
    }

    fs::create_dir_all(patch_out_dir)?;
    let stage_root = patch_out_dir.join("_stage");
    let mut written = Vec::new();

    for (archive_path, (group, changed)) in &by_archive {
        let stem = archive_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("archive");
        let stage_dir = stage_root.join(stem);
        fs::create_dir_all(&stage_dir)?;

        let mut archive = pak::PakArchive::open(archive_path)
            .with_context(|| format!("opening {}", archive_path.display()))?;
        let all_entries = archive.files();

        for e in changed {
            let original_entry = all_entries
                .iter()
                .find(|f| f.path == e.entry_path)
                .ok_or_else(|| anyhow!("entry {} not found in {}", e.entry_path, archive_path.display()))?;
            let original_bytes = archive.read_file(original_entry)?;
            let original_dds = ximg::extract_dds(&original_bytes)?;
            let format = dds::resolve_format(&ddsfile::Dds::read(original_dds)?)
                .ok_or_else(|| anyhow!("unrecognized original pixel format for {}", e.entry_path))?;

            let png_full = edited_dir.join(&e.png_rel);
            let img = image::ImageReader::open(&png_full)?.decode()?.to_rgba8();
            let (width, height) = img.dimensions();
            let new_dds = dds::encode(width, height, img.as_raw(), format)?;

            let opts = ximg::ReplaceOptions {
                width: width as i32,
                height: height as i32,
                skip_mips: None,
                pixel_format: None,
            };
            let patched = ximg::replace_dds(&original_bytes, opts, &new_dds)?;

            let rel_inside = e.entry_path.trim_start_matches('/');
            let dest = stage_dir.join(rel_inside);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest, &patched)?;
        }

        let patch_path = next_patch_path(archive_path, patch_out_dir, group)?;
        if let Some(parent) = patch_path.parent() {
            fs::create_dir_all(parent)?;
        }
        pak::write_archive_from_dir(&stage_dir, &patch_path)?;
        written.push(patch_path);
    }

    fs::remove_dir_all(&stage_root).ok();
    Ok(written)
}

/// One row of `manifest.tsv`, reshaped for UI consumption: which archive/group a texture
/// came from, its path inside the archive, and where its extracted PNG lives on disk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub group: String,
    pub archive_path: String,
    pub archive_stem: String,
    pub entry_path: String,
    pub png_rel: String,
    pub name: String,
    pub folder: String,
}

/// Reads `manifest.tsv` from a prior `extract_all` run and returns every texture in a shape
/// the UI can group into a folder tree / grid without re-parsing the raw manifest itself.
pub fn list_library(out_dir: &Path) -> Result<Vec<LibraryEntry>> {
    let manifest_path = out_dir.join(MANIFEST_NAME);
    let entries = parse_manifest(&manifest_path)?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let archive_stem = e
                .archive
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("archive")
                .to_string();
            let name = Path::new(&e.entry_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&e.entry_path)
                .to_string();
            let folder = Path::new(&e.entry_path)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let folder = folder.trim_start_matches('/').to_string();
            LibraryEntry {
                group: e.group,
                archive_path: e.archive.to_string_lossy().replace('\\', "/"),
                archive_stem,
                entry_path: e.entry_path,
                png_rel: e.png_rel,
                name,
                folder,
            }
        })
        .collect())
}

/// One real asset entry (mesh, actor/skeleton, or motion clip) found in a game archive — pure
/// metadata, no conversion has happened yet. Mirrors `LibraryEntry`'s shape so the UI can reuse
/// the same folder-tree/search components for all of these as it does for textures.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveAssetEntry {
    pub group: String,
    pub archive_path: String,
    pub archive_stem: String,
    pub entry_path: String,
    pub name: String,
    pub folder: String,
}

/// Kept as an alias: `list_meshes` predates the other asset kinds and the UI/Tauri layer
/// already names its DTO `MeshEntry` — no need to churn those call sites for a rename.
pub type MeshEntry = ArchiveAssetEntry;

/// Discovers every archive reachable from `exe_or_shortcut` and lists every entry whose path
/// ends with `extension` across all of them — instant (just reads each archive's directory, no
/// decoding, no external process). Shared by `list_meshes`/`list_actors`/`list_motions`.
fn list_by_extension(exe_or_shortcut: &Path, extension: &str) -> Result<Vec<ArchiveAssetEntry>> {
    let exe = gamepath::resolve_shortcut(exe_or_shortcut)?;
    let root = gamepath::discover_game_root(&exe).ok_or_else(|| {
        anyhow!("could not find a data/ folder with archives above {}", exe.display())
    })?;
    let archives = gamepath::discover_archives(&root)?;

    let mut out = Vec::new();
    for archive_info in &archives {
        let archive = match pak::PakArchive::open(&archive_info.path) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("skipping archive {}: {e}", archive_info.path.display());
                continue;
            }
        };
        let archive_stem = archive_info
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("archive")
            .to_string();

        for entry in &archive.files() {
            if entry.is_deleted() || !entry.path.to_ascii_lowercase().ends_with(extension) {
                continue;
            }
            let name = Path::new(&entry.path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&entry.path)
                .to_string();
            let folder = Path::new(&entry.path)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let folder = folder.trim_start_matches('/').to_string();
            out.push(ArchiveAssetEntry {
                group: archive_info.group.clone(),
                archive_path: archive_info.path.to_string_lossy().replace('\\', "/"),
                archive_stem: archive_stem.clone(),
                entry_path: entry.path.clone(),
                name,
                folder,
            });
        }
    }
    Ok(out)
}

/// Lists every real `._xmsh` mesh. Meshes are converted to `.obj` lazily, one at a time, only
/// when a user actually opens one (see `mesh_to_obj_from_archive`) — spawning
/// `mimicry-helper.exe` for all ~1700 real meshes up front would be needlessly slow and, per
/// past experience with full-batch extraction runs, fragile.
pub fn list_meshes(exe_or_shortcut: &Path) -> Result<Vec<ArchiveAssetEntry>> {
    list_by_extension(exe_or_shortcut, "._xmsh")
}

/// Lists every real `._xmac` actor (skeleton + bind-pose mesh + materials for one character —
/// see `content::actor_to_obj`). Same lazy-conversion rationale as `list_meshes`.
pub fn list_actors(exe_or_shortcut: &Path) -> Result<Vec<ArchiveAssetEntry>> {
    list_by_extension(exe_or_shortcut, "._xmac")
}

/// Lists every real `._xmot` motion clip (an animation for some character, e.g.
/// `Hero_Stand_None_None_P0_Ambient_Loop...`). Keyframe playback isn't implemented yet — this
/// only exposes the real clip names/paths for browsing (see risenlab-presentation-deadline
/// memory for the reverse-engineering status of the `.xmot` keyframe format itself).
pub fn list_motions(exe_or_shortcut: &Path) -> Result<Vec<ArchiveAssetEntry>> {
    list_by_extension(exe_or_shortcut, "._xmot")
}

/// Pulls one entry's raw bytes straight out of an archive and stages them to a temp file under
/// `out_dir` (mimicry-helper needs a real file path, not bytes) — shared by
/// `mesh_to_obj_from_archive`/`actor_to_obj_from_archive`.
fn stage_raw_entry(archive_path: &Path, entry_path: &str, out_dir: &Path, raw_extension: &str) -> Result<PathBuf> {
    let mut archive = pak::PakArchive::open(archive_path)
        .with_context(|| format!("opening {}", archive_path.display()))?;
    let entry = archive
        .files()
        .into_iter()
        .find(|f| f.path == entry_path)
        .ok_or_else(|| anyhow!("entry {entry_path} not found in {}", archive_path.display()))?;
    let raw = archive.read_file(&entry)?;

    let staging_dir = out_dir.join("_raw_stage");
    fs::create_dir_all(&staging_dir)?;
    let raw_path = staging_dir.join(format!("{:016x}.{raw_extension}", fnv1a(entry_path.as_bytes())));
    fs::write(&raw_path, &raw)?;
    Ok(raw_path)
}

/// Converts one real mesh entry to `.obj` on demand: pulls its raw `._xmsh` bytes straight out
/// of the archive, stages them to a temp file, runs the conversion, and caches the result under
/// `out_dir` so re-opening the same mesh later is instant. Returns the `.obj` path.
pub fn mesh_to_obj_from_archive(archive_path: &Path, entry_path: &str, out_dir: &Path) -> Result<PathBuf> {
    let rel_inside = entry_path.trim_start_matches('/');
    let obj_path = out_dir.join(rel_inside).with_extension("obj");
    // Also require the sibling .mtl to exist: an .obj cached from before mimicry-helper started
    // writing one (or from an older mimicry-helper.exe build) would otherwise look "already
    // converted" forever and silently never pick up real texture references.
    if obj_path.exists() && obj_path.with_extension("mtl").exists() {
        return Ok(obj_path);
    }

    let raw_path = stage_raw_entry(archive_path, entry_path, out_dir, "xmsh")?;
    if let Some(parent) = obj_path.parent() {
        fs::create_dir_all(parent)?;
    }
    content::mesh_to_obj(&raw_path, &obj_path)?;
    fs::remove_file(&raw_path).ok();

    Ok(obj_path)
}

/// Converts one real actor entry (`._xmac`: skeleton + bind-pose mesh + materials for one
/// character) to `.obj` on demand, same lazy/caching approach as `mesh_to_obj_from_archive`.
/// The exported OBJ carries the bind-pose geometry only (no keyframe animation — see
/// `list_motions`'s doc comment for that status).
pub fn actor_to_obj_from_archive(archive_path: &Path, entry_path: &str, out_dir: &Path) -> Result<PathBuf> {
    let rel_inside = entry_path.trim_start_matches('/');
    let obj_path = out_dir.join(rel_inside).with_extension("obj");
    // See the matching comment in mesh_to_obj_from_archive: an .obj cached from before
    // mimicry-helper wrote a sibling .mtl must not be treated as already up to date.
    if obj_path.exists() && obj_path.with_extension("mtl").exists() {
        return Ok(obj_path);
    }

    let raw_path = stage_raw_entry(archive_path, entry_path, out_dir, "xmac")?;
    if let Some(parent) = obj_path.parent() {
        fs::create_dir_all(parent)?;
    }
    content::actor_to_obj(&raw_path, &obj_path)?;
    fs::remove_file(&raw_path).ok();

    Ok(obj_path)
}

/// Pulls one archive entry's raw (already-decompressed) bytes straight into memory — for
/// formats parsed directly in Rust (`xmac`/`xmot`), which don't need a real file path the way
/// `mimicry-helper.exe` does (see `stage_raw_entry`).
fn read_raw_entry_bytes(archive_path: &Path, entry_path: &str) -> Result<Vec<u8>> {
    let mut archive = pak::PakArchive::open(archive_path)
        .with_context(|| format!("opening {}", archive_path.display()))?;
    let entry = archive
        .files()
        .into_iter()
        .find(|f| f.path == entry_path)
        .ok_or_else(|| anyhow!("entry {entry_path} not found in {}", archive_path.display()))?;
    Ok(archive.read_file(&entry)?)
}

/// A real actor's bone hierarchy (name, parent link, bind-pose local transform) — parsed
/// directly from the `._xmac` bytes in Rust (`xmac::parse_skeleton`), independent of
/// `mimicry-helper.exe`/its OBJ export (which has no way to carry skinning/hierarchy data
/// anyway — OBJ is a static-mesh format). Used to drive keyframe playback (`motion_tracks`)
/// without needing any mesh conversion at all.
pub fn actor_skeleton(archive_path: &Path, entry_path: &str) -> Result<Vec<xmac::SkeletonNode>> {
    let data = read_raw_entry_bytes(archive_path, entry_path)?;
    xmac::parse_skeleton(&data)
}

/// A real motion clip's per-bone position/rotation/scale keyframe tracks (`xmot::parse_motion`)
/// for each name in `bone_names` — typically every bone from the matching actor's
/// `actor_skeleton`, so bones this clip doesn't animate simply come back empty.
pub fn motion_tracks(archive_path: &Path, entry_path: &str, bone_names: &[String]) -> Result<Vec<xmot::BoneMotion>> {
    let data = read_raw_entry_bytes(archive_path, entry_path)?;
    xmot::parse_motion(&data, bone_names)
}

/// A real actor's skinned mesh (positions/normals/UVs/faces/per-vertex bone weights), parsed
/// directly from the `._xmac` bytes in Rust (`xmesh_skin::parse_skinned_mesh`) — the data
/// `mesh_to_obj_from_archive`'s OBJ export can't carry (OBJ has no per-vertex bone weights),
/// needed to actually deform the mesh surface with `actor_skeleton`/`motion_tracks` instead of
/// only animating a bare bone hierarchy.
pub fn actor_skinned_mesh(archive_path: &Path, entry_path: &str) -> Result<xmesh_skin::SkinnedMesh> {
    let data = read_raw_entry_bytes(archive_path, entry_path)?;
    xmesh_skin::parse_skinned_mesh(&data)
}

/// Which real texture file names a mesh/actor's own material(s) reference — straight from the
/// game data, not a name-matching guess. `mimicry-helper`'s `mesh-to-obj`/`actor-to-obj` now
/// also write a sibling `.mtl` next to the `.obj` (see `mimicry-helper/driver/main.cpp`), whose
/// `map_Kd`/`map_bump` lines carry the exact texture file name each material points at (as
/// referenced by the game's own dev-time material setup, e.g. `ItWpn_SwordBlades_01_Diffuse_01`
/// — matched against the real library by base name since the extension convention differs).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialTextureRefs {
    pub diffuse: Option<String>,
    pub normal: Option<String>,
}

/// One material's own explicit texture map references, parsed from a `.mtl` block (`newmtl`
/// through the next `newmtl`/EOF).
#[derive(Default)]
struct MtlMaterial {
    map_kd: Option<String>,
    map_bump: Option<String>,
}

/// Parses every `newmtl <name>` block in a `.mtl` file into a name -> declared texture maps
/// table. Real Genome-exported materials often have NO `map_Kd`/`map_bump` line at all (the
/// engine looks the texture up by the material's own name instead) — callers must be ready for
/// both `map_kd`/`map_bump` to be `None` even for a real, in-use material.
fn parse_mtl(text: &str) -> Vec<(String, MtlMaterial)> {
    let mut materials: Vec<(String, MtlMaterial)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("newmtl ") {
            materials.push((name.trim().to_string(), MtlMaterial::default()));
        } else if let Some(rest) = line.strip_prefix("map_Kd ") {
            if let Some((_, m)) = materials.last_mut() {
                m.map_kd = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = line.strip_prefix("map_bump ") {
            if let Some((_, m)) = materials.last_mut() {
                m.map_bump = Some(rest.trim().to_string());
            }
        }
    }
    materials
}

/// Parses a `.obj` `f ` line's vertex indices (the first number in each `v/vt/vn` triple,
/// tolerating faces with only `v` or `v/vt`), 1-indexed as OBJ always is.
fn parse_face_vertex_indices(line: &str) -> Vec<usize> {
    line.split_whitespace()
        .skip(1) // "f"
        .filter_map(|token| token.split('/').next()?.parse::<usize>().ok())
        .collect()
}

/// Finds which material actually covers the most of the mesh's *visible surface area* — not
/// just face count. A mesh commonly has several sub-materials left over from a shared
/// multi-material template (e.g. a sword's simple, large blade vs. a small but
/// triangle-dense/ornate hilt or guard); picking by raw face count wrongly favors whichever
/// part happens to be modeled with more, smaller triangles, even if it covers far less of what
/// you'd actually look at (confirmed on a real sword: a detailed hilt/misc material had more
/// faces than the blade itself, but the blade is what dominates the silhouette). Real triangle
/// area, computed from the actual vertex positions, is what "the texture you'd notice most"
/// actually means.
fn triangle_area(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt()
}

fn primary_material_by_surface_area(obj_text: &str) -> Option<String> {
    let mut vertices: Vec<[f64; 3]> = Vec::new();
    let mut areas: HashMap<String, f64> = HashMap::new();
    let mut current: Option<&str> = None;

    for line in obj_text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("v ") {
            let coords: Vec<f64> = rest.split_whitespace().filter_map(|s| s.parse().ok()).collect();
            if let [x, y, z] = coords[..] {
                vertices.push([x, y, z]);
            }
        } else if let Some(name) = line.strip_prefix("usemtl ") {
            current = Some(name.trim());
        } else if line.starts_with("f ") {
            let Some(name) = current else { continue };
            let indices = parse_face_vertex_indices(line);
            let points: Option<Vec<[f64; 3]>> =
                indices.iter().map(|&i| i.checked_sub(1).and_then(|i| vertices.get(i)).copied()).collect();
            let Some(points) = points else { continue };
            // Fan-triangulate in case of an n-gon (real exports are usually triangles already).
            for pair in points[1..].windows(2) {
                let area = triangle_area(points[0], pair[0], pair[1]);
                *areas.entry(name.to_string()).or_insert(0.0) += area;
            }
        }
    }
    areas.into_iter().max_by(|(_, a), (_, b)| a.total_cmp(b)).map(|(name, _)| name)
}

/// Guesses a normal-map file name from a diffuse one by swapping the naming-convention token —
/// real texture pairs in this game differ only by this (e.g. `..._Diffuse_01` / `..._Normal_01`
/// — confirmed on real extracted textures). Best-effort only: the caller looks the guess up
/// against the real library and simply gets nothing back if it doesn't exist.
fn guess_normal_from_diffuse_name(diffuse: &str) -> Option<String> {
    for (from, to) in [("_Diffuse_", "_Normal_"), ("_diffuse_", "_normal_"), ("Diffuse", "Normal")] {
        if diffuse.contains(from) {
            return Some(diffuse.replacen(from, to, 1));
        }
    }
    None
}

/// Which real texture file names a mesh/actor's own material(s) reference — straight from the
/// game data, not a name-matching guess. `mimicry-helper`'s `mesh-to-obj`/`actor-to-obj` now
/// also write a sibling `.mtl` next to the `.obj` (see `mimicry-helper/driver/main.cpp`). Picks
/// the material covering the most of the mesh (see `primary_material_by_surface_area`) since real
/// meshes often carry several unused/partial sub-materials; prefers that material's explicit
/// `map_Kd`/`map_bump` if present, otherwise falls back to the material's own name (real Genome
/// materials are frequently named after their texture directly, e.g. `ItWpn_SwordBlades_01_
/// Diffuse_01` with no explicit map line at all — confirmed on real extracted meshes) and a
/// best-effort normal-map name guess from that. Returns both as `None` if there's no `.mtl`/
/// `.obj` (e.g. an older cached `.obj` from before this existed) rather than erroring — this is
/// a nice-to-have auto-match, not something the rest of the pipeline depends on.
pub fn read_material_texture_refs(obj_path: &Path) -> Result<MaterialTextureRefs> {
    let mtl_path = obj_path.with_extension("mtl");
    let mtl_text = fs::read_to_string(&mtl_path).unwrap_or_default();
    let obj_text = fs::read_to_string(obj_path).unwrap_or_default();
    let materials = parse_mtl(&mtl_text);

    let primary_name = primary_material_by_surface_area(&obj_text).or_else(|| materials.first().map(|(n, _)| n.clone()));
    let Some(primary_name) = primary_name else {
        return Ok(MaterialTextureRefs { diffuse: None, normal: None });
    };
    let primary = materials.iter().find(|(n, _)| *n == primary_name).map(|(_, m)| m);

    let diffuse = primary.and_then(|m| m.map_kd.clone()).or_else(|| Some(primary_name.clone()));
    let normal = primary
        .and_then(|m| m.map_bump.clone())
        .or_else(|| diffuse.as_deref().and_then(guess_normal_from_diffuse_name));

    Ok(MaterialTextureRefs { diffuse, normal })
}

/// Upscales an already-extracted PNG via Lanczos3 resize and writes the result into an
/// `edited/` sibling directory, ready for review/`apply`. This is today's real "AI
/// regenerate" capability — the same placeholder pipeline proven end-to-end on the real
/// game (see `docs/ROADMAP.md`'s "Full texture pipeline round trip" entry), generalized from
/// a one-off proof into a reusable function. A real ML upscaler is a separate, not-yet-
/// approved decision (see `docs/ROADMAP.md`'s "Next" section) — this is not it.
pub fn regenerate(out_dir: &Path, png_rel: &str, scale: u32) -> Result<PathBuf> {
    let src = out_dir.join(png_rel);
    let img = image::ImageReader::open(&src)
        .with_context(|| format!("opening {}", src.display()))?
        .decode()?
        .to_rgba8();
    let (width, height) = img.dimensions();
    let scale = scale.max(1);
    let resized = image::imageops::resize(
        &img,
        width * scale,
        height * scale,
        image::imageops::FilterType::Lanczos3,
    );

    let dest = out_dir.join("edited").join(png_rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    resized
        .save(&dest)
        .with_context(|| format!("writing {}", dest.display()))?;
    Ok(dest)
}

/// Real per-texture metadata (dimensions/format/size) read straight from the source archive
/// entry — used by the review UI's detail panel. `entry_path` must match a `LibraryEntry`'s
/// `entry_path` exactly (as recorded in the manifest).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureMeta {
    pub width: i32,
    pub height: i32,
    pub pixel_format: String,
    pub file_size: u64,
}

pub fn texture_meta(archive_path: &Path, entry_path: &str) -> Result<TextureMeta> {
    let mut archive =
        pak::PakArchive::open(archive_path).with_context(|| format!("opening {}", archive_path.display()))?;
    let entries = archive.files();
    let entry = entries
        .iter()
        .find(|f| f.path == entry_path)
        .ok_or_else(|| anyhow!("entry {entry_path} not found in {}", archive_path.display()))?;
    let data = archive.read_file(entry)?;
    let info = ximg::parse(&data).map_err(anyhow::Error::from)?;
    let pixel_format = ximg::read_pixel_format(&data).unwrap_or_else(|_| "?".to_string());
    Ok(TextureMeta {
        width: info.width,
        height: info.height,
        pixel_format,
        file_size: data.len() as u64,
    })
}

const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(BASE64_ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(BASE64_ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 { BASE64_ALPHABET[((n >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { BASE64_ALPHABET[(n & 0x3F) as usize] as char } else { '=' });
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Builds a single, self-contained HTML page (images embedded as base64 data URIs, no
/// external files) showing original-vs-edited side by side for every PNG that actually
/// changed since extraction — a cheap stand-in for a dedicated review UI: open it in any
/// browser before running `apply` to see exactly what's about to be patched.
pub fn build_review_html(manifest_path: &Path, edited_dir: &Path, out_html: &Path) -> Result<usize> {
    let entries = parse_manifest(manifest_path)?;
    let original_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));

    let mut rows = String::new();
    let mut changed_count = 0usize;
    for e in &entries {
        let edited_path = edited_dir.join(&e.png_rel);
        if !edited_path.exists() {
            continue;
        }
        let edited_bytes = fs::read(&edited_path)?;
        if fnv1a(&edited_bytes) == e.hash {
            continue; // unchanged since extraction
        }
        changed_count += 1;

        let original_path = original_dir.join(&e.png_rel);
        let original_bytes = fs::read(&original_path).unwrap_or_default();

        rows.push_str(&format!(
            r#"<div class="row"><h3>{}</h3><div class="pair">
<figure><img src="data:image/png;base64,{}"><figcaption>original</figcaption></figure>
<figure><img src="data:image/png;base64,{}"><figcaption>edited</figcaption></figure>
</div></div>
"#,
            html_escape(&e.entry_path),
            base64_encode(&original_bytes),
            base64_encode(&edited_bytes),
        ));
    }

    let html = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>RisenLab texture review</title>
<style>
body {{ font-family: sans-serif; background: #111; color: #eee; padding: 1rem; }}
.row {{ margin-bottom: 2rem; border-bottom: 1px solid #333; padding-bottom: 1rem; }}
.pair {{ display: flex; gap: 1rem; flex-wrap: wrap; }}
figure {{ margin: 0; }}
img {{ max-width: 400px; max-height: 400px; image-rendering: pixelated; border: 1px solid #444; }}
figcaption {{ text-align: center; color: #999; }}
</style></head>
<body>
<h1>{changed_count} changed texture(s)</h1>
{rows}
</body></html>
"#
    );

    fs::write(out_html, html)?;
    Ok(changed_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_prop(buf: &mut Vec<u8>, name: &str, type_name: &str, value: &[u8]) {
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&(type_name.len() as u16).to_le_bytes());
        buf.extend_from_slice(type_name.as_bytes());
        buf.extend_from_slice(&30u16.to_le_bytes());
        buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
        buf.extend_from_slice(value);
    }

    /// Same synthetic `._ximg` layout `ximg.rs`'s own tests use — duplicated here (rather
    /// than exported cross-module) since it's test-only fixture code.
    fn synthetic_ximg(width: i32, height: i32, pixel_format: &str) -> Vec<u8> {
        let mut props = Vec::new();
        push_prop(&mut props, "Width", "int", &width.to_le_bytes());
        push_prop(&mut props, "Height", "int", &height.to_le_bytes());
        push_prop(&mut props, "SkipMips", "long", &0i32.to_le_bytes());
        let mut fmt_value = vec![0xC9, 0x00];
        fmt_value.extend_from_slice(pixel_format.as_bytes());
        push_prop(&mut props, "PixelFormat", "bTPropertyContainer<enum eCGfxShared::eEColorFormat>", &fmt_value);

        let dds_offset = 20 + props.len();
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GR01IM04");
        buf.extend_from_slice(&40i32.to_le_bytes());
        buf.extend_from_slice(&(props.len() as i32).to_le_bytes());
        buf.extend_from_slice(&(dds_offset as i32).to_le_bytes());
        buf.extend_from_slice(&props);
        buf.extend_from_slice(b"DDS ");
        buf.extend_from_slice(&[0u8; 16]);
        buf
    }

    fn texture_meta_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("risenlab_texmeta_test_{tag}_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn texture_meta_reads_real_width_height_format_from_a_packed_archive() {
        let dir = texture_meta_temp_dir("ok");
        let src_dir = dir.join("src");
        fs::create_dir_all(src_dir.join("Level")).unwrap();
        let ximg_bytes = synthetic_ximg(64, 32, "DXT3");
        fs::write(src_dir.join("Level").join("Test._ximg"), &ximg_bytes).unwrap();

        let archive_path = dir.join("images.pak");
        pak::write_archive_from_dir(&src_dir, &archive_path).unwrap();

        let archive = pak::PakArchive::open(&archive_path).unwrap();
        let entry_path = archive.files()[0].path.clone();

        let meta = texture_meta(&archive_path, &entry_path).unwrap();
        assert_eq!(meta.width, 64);
        assert_eq!(meta.height, 32);
        assert_eq!(meta.pixel_format, "DXT3");
        assert_eq!(meta.file_size, ximg_bytes.len() as u64);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn texture_meta_errors_when_entry_not_found() {
        let dir = texture_meta_temp_dir("missing_entry");
        let src_dir = dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("real.txt"), b"x").unwrap();
        let archive_path = dir.join("archive.pak");
        pak::write_archive_from_dir(&src_dir, &archive_path).unwrap();

        assert!(texture_meta(&archive_path, "does/not/exist._ximg").is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn texture_meta_errors_when_archive_does_not_exist() {
        let dir = texture_meta_temp_dir("missing_archive");
        assert!(texture_meta(&dir.join("nope.pak"), "x").is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn fnv1a_is_deterministic_and_sensitive_to_content() {
        assert_eq!(fnv1a(b"hello"), fnv1a(b"hello"));
        assert_ne!(fnv1a(b"hello"), fnv1a(b"hellp"));
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "risenlab_batch_test_{tag}_{}_{}",
            std::process::id(),
            fnv1a(tag.as_bytes())
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn list_library_groups_and_names_entries_from_a_manifest() {
        let out_dir = temp_dir("list_library");
        let mut manifest = fs::File::create(out_dir.join(MANIFEST_NAME)).unwrap();
        write_manifest_line(
            &mut manifest,
            &ManifestEntry {
                archive: PathBuf::from("C:/Game/data/compiled/images.pak"),
                group: "compiled".to_string(),
                entry_path: "Level/Nat_Rock/Nat_Stone_Rock_01._ximg".to_string(),
                png_rel: "compiled/images/Level/Nat_Rock/Nat_Stone_Rock_01.png".to_string(),
                hash: 0xdead_beef,
            },
        )
        .unwrap();
        drop(manifest);

        let entries = list_library(&out_dir).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.group, "compiled");
        assert_eq!(e.archive_stem, "images");
        assert_eq!(e.archive_path, "C:/Game/data/compiled/images.pak");
        assert_eq!(e.name, "Nat_Stone_Rock_01._ximg");
        assert_eq!(e.folder, "Level/Nat_Rock");
        assert_eq!(e.png_rel, "compiled/images/Level/Nat_Rock/Nat_Stone_Rock_01.png");

        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn list_library_strips_leading_slash_from_folder_like_real_archive_entries() {
        // Real Risen archive entry paths come back from the pak reader with a leading "/"
        // (e.g. "/Animation/Monster/Foo._ximg"), so `Path::parent()` also keeps that leading
        // slash on `folder` ("/Animation/Monster") — this must be stripped, or every consumer
        // that keys off the first path segment (the UI's folder tree) silently sees an empty
        // top-level segment for every single real entry.
        let out_dir = temp_dir("list_library_leading_slash");
        let mut manifest = fs::File::create(out_dir.join(MANIFEST_NAME)).unwrap();
        write_manifest_line(
            &mut manifest,
            &ManifestEntry {
                archive: PathBuf::from("C:/Game/data/compiled/images.pak"),
                group: "compiled".to_string(),
                entry_path: "/Animation/Monster/Foo._ximg".to_string(),
                png_rel: "compiled/images/Animation/Monster/Foo.png".to_string(),
                hash: 0xdead_beef,
            },
        )
        .unwrap();
        drop(manifest);

        let entries = list_library(&out_dir).unwrap();
        assert_eq!(entries[0].folder, "Animation/Monster");

        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn list_library_errors_when_no_manifest_present() {
        let out_dir = temp_dir("list_library_missing");
        assert!(list_library(&out_dir).is_err());
        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn read_material_texture_refs_parses_map_kd_and_map_bump_from_a_real_shaped_mtl() {
        let out_dir = temp_dir("material_refs");
        let obj_path = out_dir.join("It_Wpn_BS_TitanSword.obj");
        // Real shape written by mCObjWriter::WriteMaterial (mi_objwriter.cpp): "newmtl <name>"
        // then "map_Kd <file>" / "map_bump <file>" / "map_Ks <file>" lines, blank line, CRLF.
        fs::write(
            out_dir.join("It_Wpn_BS_TitanSword.mtl"),
            "newmtl ItWpn_SwordBlades_01\r\nmap_Kd ItWpn_SwordBlades_01_Diffuse_01.tga\r\nmap_bump ItWpn_SwordBlades_01_Normal_01.tga\r\n\r\n",
        )
        .unwrap();

        let refs = read_material_texture_refs(&obj_path).unwrap();
        assert_eq!(refs.diffuse.as_deref(), Some("ItWpn_SwordBlades_01_Diffuse_01.tga"));
        assert_eq!(refs.normal.as_deref(), Some("ItWpn_SwordBlades_01_Normal_01.tga"));

        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn read_material_texture_refs_picks_the_material_covering_the_most_surface_area() {
        // Real bug found on a real sword: a small, triangle-DENSE hilt/misc material had MORE
        // faces than the large, simple blade, even though the blade covers far more of what
        // you'd actually see — picking by raw face count wrongly chose the hilt. Here: 20 tiny
        // faces (a 1x1 unit square, fan-triangulated) for "Misc" vs. 2 faces forming one huge
        // 100x100 quad for "Blades" — by count Misc wins (20 > 2), by area Blades must win.
        let out_dir = temp_dir("material_refs_area");
        let obj_path = out_dir.join("It_Wpn_BS_TitanSword.obj");
        fs::write(
            out_dir.join("It_Wpn_BS_TitanSword.mtl"),
            "newmtl ItWpn_SwordMisc_01_Diffuse_01\r\n\r\nnewmtl ItWpn_SwordBlades_01_Diffuse_01\r\n\r\n",
        )
        .unwrap();

        let mut obj = String::from("o TitanSword\n");
        // Vertices 1-4: a tiny 1x1 quad (for the hilt/misc material).
        obj += "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\n";
        // Vertices 5-8: one huge 100x100 quad (for the blade material).
        obj += "v 0 0 10\nv 100 0 10\nv 100 100 10\nv 0 100 10\n";
        obj += "usemtl ItWpn_SwordMisc_01_Diffuse_01\n";
        for _ in 0..20 {
            obj += "f 1 2 3\n"; // 20 tiny triangles, area 0.5 each = 10 total
        }
        obj += "usemtl ItWpn_SwordBlades_01_Diffuse_01\n";
        obj += "f 5 6 7\nf 5 7 8\n"; // 2 triangles covering the 100x100 quad = 10000 total

        fs::write(&obj_path, obj).unwrap();

        let refs = read_material_texture_refs(&obj_path).unwrap();
        // No explicit map_Kd anywhere, so falls back to the winning material's own name.
        assert_eq!(refs.diffuse.as_deref(), Some("ItWpn_SwordBlades_01_Diffuse_01"));
        // Best-effort normal guess: swap the naming-convention token.
        assert_eq!(refs.normal.as_deref(), Some("ItWpn_SwordBlades_01_Normal_01"));

        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn read_material_texture_refs_returns_none_when_no_mtl_file_exists() {
        let out_dir = temp_dir("material_refs_missing");
        let obj_path = out_dir.join("no_material_here.obj");

        let refs = read_material_texture_refs(&obj_path).unwrap();
        assert_eq!(refs.diffuse, None);
        assert_eq!(refs.normal, None);

        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn regenerate_upscales_a_png_by_the_requested_scale_into_edited_dir() {
        let out_dir = temp_dir("regenerate");
        let png_rel = "compiled/images/Level/rock.png";
        let src = out_dir.join(png_rel);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        let img = image::RgbaImage::from_pixel(8, 4, image::Rgba([10, 20, 30, 255]));
        img.save(&src).unwrap();

        let dest = regenerate(&out_dir, png_rel, 2).unwrap();
        assert_eq!(dest, out_dir.join("edited").join(png_rel));
        let decoded = image::ImageReader::open(&dest).unwrap().decode().unwrap();
        assert_eq!(decoded.width(), 16);
        assert_eq!(decoded.height(), 8);

        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn regenerate_treats_scale_zero_as_scale_one() {
        let out_dir = temp_dir("regenerate_scale0");
        let png_rel = "img.png";
        let src = out_dir.join(png_rel);
        image::RgbaImage::from_pixel(5, 5, image::Rgba([1, 2, 3, 255]))
            .save(&src)
            .unwrap();

        let dest = regenerate(&out_dir, png_rel, 0).unwrap();
        let decoded = image::ImageReader::open(&dest).unwrap().decode().unwrap();
        assert_eq!((decoded.width(), decoded.height()), (5, 5));

        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn regenerate_errors_when_source_png_is_missing() {
        let out_dir = temp_dir("regenerate_missing");
        assert!(regenerate(&out_dir, "nope.png", 2).is_err());
        fs::remove_dir_all(&out_dir).ok();
    }

    /// Builds a synthetic game install with a real `meshes.pak` (via `pak::write_archive_from_dir`,
    /// so entry paths get the real leading-slash format) containing a mix of `._xmsh` meshes
    /// (one nested, one top-level) and an unrelated non-mesh file, plus a real `images.pak` in
    /// `compiled/` to prove archives are filtered correctly by content, not just by name.
    #[test]
    fn list_meshes_finds_only_xmsh_entries_across_all_archives_with_correct_folders() {
        let tmp = temp_dir("list_meshes_install");
        let bin_dir = tmp.join("bin");
        let common_dir = tmp.join("data").join("common");
        let compiled_dir = tmp.join("data").join("compiled");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&common_dir).unwrap();
        fs::create_dir_all(&compiled_dir).unwrap();

        let mesh_src = tmp.join("mesh_src");
        fs::create_dir_all(mesh_src.join("Animation").join("Monster")).unwrap();
        fs::write(mesh_src.join("UI_Crosshair._xmsh"), b"top-level mesh").unwrap();
        fs::write(mesh_src.join("Animation").join("Monster").join("Wolf._xmsh"), b"nested mesh").unwrap();
        fs::write(mesh_src.join("compiled_meshes.bin"), b"not a mesh entry").unwrap();
        pak::write_archive_from_dir(&mesh_src, &common_dir.join("meshes.pak")).unwrap();

        let material_src = tmp.join("material_src");
        fs::create_dir_all(&material_src).unwrap();
        fs::write(material_src.join("Foo._xmat"), b"not a mesh archive at all").unwrap();
        pak::write_archive_from_dir(&material_src, &compiled_dir.join("images.pak")).unwrap();

        let exe = bin_dir.join("Risen.exe");
        fs::write(&exe, b"x").unwrap();

        let entries = list_meshes(&exe).unwrap();
        assert_eq!(entries.len(), 2, "expected only the two ._xmsh entries, got {entries:?}");

        let top = entries.iter().find(|e| e.name == "UI_Crosshair._xmsh").unwrap();
        assert_eq!(top.folder, "");
        assert_eq!(top.group, "common");
        assert_eq!(top.archive_stem, "meshes");
        assert_eq!(top.entry_path, "/UI_Crosshair._xmsh");

        let nested = entries.iter().find(|e| e.name == "Wolf._xmsh").unwrap();
        assert_eq!(nested.folder, "Animation/Monster");

        fs::remove_dir_all(&tmp).ok();
    }
}
