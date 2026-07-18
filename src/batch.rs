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
    let mut manifest = std::io::BufWriter::new(fs::File::create(out_dir.join(MANIFEST_NAME))?);

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
            // Encode to an in-memory buffer once, then both write it and hash it from the same
            // bytes — this used to be `img.save()` followed by `fs::read()` of what was just
            // written, a full extra disk round trip per texture (1342 of them on a real
            // extraction run) purely to get bytes already available in memory.
            let mut png_bytes = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
                .with_context(|| format!("encoding {}", entry.path))?;
            fs::write(&png_full, &png_bytes).with_context(|| format!("writing {}", png_full.display()))?;

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
    manifest.flush()?;
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

/// Copies every built patch volume from `patch_dir` (`<group>/<archive>.pNN` — the layout
/// `apply`/`pack` produce) into the game's own matching `data/<group>/` directory, next to the
/// archive it patches. Never overwrites a file that already exists in the game dir with a
/// DIFFERENT origin: only a byte-different same-named file is replaced (same-named = ours from
/// a previous install — patch slots are allocated to be free at build time). Returns the
/// installed file names, grouped.
pub fn install_patches(exe_or_shortcut: &Path, patch_dir: &Path) -> Result<Vec<String>> {
    let exe = gamepath::resolve_shortcut(exe_or_shortcut)?;
    let root = gamepath::discover_game_root(&exe)
        .ok_or_else(|| anyhow!("no Risen data directory found near {}", exe.display()))?;
    let mut installed = Vec::new();
    for group in ["compiled", "common"] {
        let src_group = patch_dir.join(group);
        if !src_group.is_dir() {
            continue;
        }
        let dest_group = root.join("data").join(group);
        if !dest_group.is_dir() {
            bail!("game data directory missing: {}", dest_group.display());
        }
        for entry in fs::read_dir(&src_group)? {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else { continue };
            // Only .pNN patch volumes — never copy anything else that might sit in the folder.
            let is_patch = Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.len() >= 2 && e.starts_with('p') && e[1..].chars().all(|c| c.is_ascii_digit()))
                .unwrap_or(false);
            if !path.is_file() || !is_patch {
                continue;
            }
            fs::copy(&path, dest_group.join(&name))
                .with_context(|| format!("copying {name} into {}", dest_group.display()))?;
            installed.push(format!("{group}/{name}"));
        }
    }
    Ok(installed)
}

/// Removes previously installed patch volumes from the game: only files whose exact name also
/// exists in `patch_dir` (i.e. files this app built) are deleted — nothing else in the game
/// directory is ever touched. Returns the removed file names.
pub fn uninstall_patches(exe_or_shortcut: &Path, patch_dir: &Path) -> Result<Vec<String>> {
    let exe = gamepath::resolve_shortcut(exe_or_shortcut)?;
    let root = gamepath::discover_game_root(&exe)
        .ok_or_else(|| anyhow!("no Risen data directory found near {}", exe.display()))?;
    let mut removed = Vec::new();
    for group in ["compiled", "common"] {
        let src_group = patch_dir.join(group);
        if !src_group.is_dir() {
            continue;
        }
        let dest_group = root.join("data").join(group);
        for entry in fs::read_dir(&src_group)? {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else { continue };
            let installed = dest_group.join(&name);
            if path.is_file() && installed.is_file() {
                fs::remove_file(&installed).with_context(|| format!("removing {}", installed.display()))?;
                removed.push(format!("{group}/{name}"));
            }
        }
    }
    Ok(removed)
}

/// The four independently-toggleable local motion transforms (`xmot::stylize_tracks` plus the
/// pre-existing jitter filter), bundled so every write-side entry point below takes one value
/// instead of a growing positional-argument list. All default to `0.0` (no-op).
#[derive(Debug, Clone, Copy, Default)]
pub struct MotionStyle {
    pub smooth: f32,
    pub expressiveness: f32,
    pub secondary: f32,
    pub sharpness: f32,
}

/// Batch counterpart of `export_motion_patch`: applies the same style to MANY clips (typically
/// every animation of one creature — they share the actor's skeleton, so one bone-name list
/// fits all) and packs them into a SINGLE `<stem>.pNN` patch volume. Clips that fail to parse
/// are skipped (returned in the error list) rather than aborting the whole creature.
pub fn export_motion_patch_batch(
    archive_path: &Path,
    entry_paths: &[String],
    bone_names: &[String],
    style: MotionStyle,
    patch_dir: &Path,
    group: &str,
) -> Result<(PathBuf, Vec<String>)> {
    let stage_dir = patch_dir.join("_motion_stage");
    let _ = fs::remove_dir_all(&stage_dir);
    let mut failed = Vec::new();
    let mut staged_any = false;
    for entry_path in entry_paths {
        let staged = stage_dir.join(entry_path.trim_start_matches('/'));
        match style_motion_to_file(archive_path, entry_path, bone_names, style, &staged) {
            Ok(()) => staged_any = true,
            Err(e) => failed.push(format!("{entry_path}: {e:#}")),
        }
    }
    if !staged_any {
        let _ = fs::remove_dir_all(&stage_dir);
        bail!("no clips could be smoothed ({} failures)", failed.len());
    }
    let patch_path = next_patch_path(archive_path, patch_dir, group)?;
    if let Some(parent) = patch_path.parent() {
        fs::create_dir_all(parent)?;
    }
    pak::write_archive_from_dir_with(&stage_dir, &patch_path, pak::FileCompression::ZLib)?;
    let _ = fs::remove_dir_all(&stage_dir);
    Ok((patch_path, failed))
}

/// The complete "enhance this animation → installable mod file" chain: styles one clip
/// (`style_motion_to_file`), stages it under its real entry path, and packs a fresh
/// `<stem>.pNN` patch volume into `patch_dir/<group>/` using the source archive family's own
/// storage convention (`animations.pak` = ZLib entries — see `pak::write_archive_from_dir_with`).
/// Returns the patch path, ready for `install_patches`.
pub fn export_motion_patch(
    archive_path: &Path,
    entry_path: &str,
    bone_names: &[String],
    style: MotionStyle,
    patch_dir: &Path,
    group: &str,
) -> Result<PathBuf> {
    let stage_dir = patch_dir.join("_motion_stage");
    let _ = fs::remove_dir_all(&stage_dir);
    let staged = stage_dir.join(entry_path.trim_start_matches('/'));
    style_motion_to_file(archive_path, entry_path, bone_names, style, &staged)?;

    let patch_path = next_patch_path(archive_path, patch_dir, group)?;
    if let Some(parent) = patch_path.parent() {
        fs::create_dir_all(parent)?;
    }
    pak::write_archive_from_dir_with(&stage_dir, &patch_path, pak::FileCompression::ZLib)?;
    let _ = fs::remove_dir_all(&stage_dir);
    Ok(patch_path)
}

/// Real, local (no external AI) motion cleanup+stylization end-to-end: reads one clip from its
/// archive, runs the jitter filter (`xmot::smooth_tracks`) and/or the three quality transforms
/// (`xmot::stylize_tracks` — amplitude boost / secondary motion / attack retiming, each
/// independently toggled by `style`), patches the key values (and, for retiming, the key
/// TIMES) back IN PLACE (`xmot::patch_motion_keys` — same counts/sizes, so the whole file
/// structure incl. every not-yet-decoded wrapper field survives byte-for-byte) and writes the
/// result to `out_path`. An all-zero `style` produces output verifiably byte-identical to the
/// original entry — the built-in correctness check for the whole read→locate→write chain.
pub fn style_motion_to_file(archive_path: &Path, entry_path: &str, bone_names: &[String], style: MotionStyle, out_path: &Path) -> Result<()> {
    let data = read_raw_entry_bytes(archive_path, entry_path)?;
    let tracks = xmot::parse_motion(&data, bone_names)?;
    let smoothed = xmot::smooth_tracks(&tracks, style.smooth);
    let styled = xmot::stylize_tracks(&smoothed, style.expressiveness, style.secondary, style.sharpness);
    let patched = xmot::patch_motion_keys(&data, &styled)?;
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_path, patched).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

/// Back-compat alias kept for the CLI's standalone `smooth-motion` command (jitter filter only,
/// no expressiveness/secondary/retiming) — see `style_motion_to_file` for the general path.
pub fn smooth_motion_to_file(archive_path: &Path, entry_path: &str, bone_names: &[String], strength: f32, out_path: &Path) -> Result<()> {
    style_motion_to_file(archive_path, entry_path, bone_names, MotionStyle { smooth: strength, ..Default::default() }, out_path)
}

/// The real, exportable counterpart of the "🎬 60fps" IN-APP PREVIEW (`xmot::resample_double_rate`
/// via `MotionStyle.doubleRate`, wired only through `motion-tracks`, never through this file):
/// doubles every bone's position/rotation key rate and writes a genuinely RESIZED `._xmot` via
/// `xmot::rebuild_motion_file` (unlike `style_motion_to_file`'s in-place patch, which can only
/// ever produce the SAME byte length). Scale/scale-rotation channels are carried through
/// untouched. `bone_names` must be the actor's COMPLETE real skeleton (every node from
/// `actor-skeleton`, not a filtered subset) — `rebuild_motion_file` walks records in file order
/// and refuses to run rather than silently drop a real record it wasn't told about.
///
/// UNVERIFIED IN-GAME: this is new, self-consistency-tested (round-trip byte-identical at
/// zero-op, correct doubled counts, size field patched) but never confirmed against the real
/// engine's own `.xmot` loader — the chunk-wrapper size field this needed decoded was reverse
/// engineered from two real clips' own declared vs. actual byte lengths, not from documentation
/// or engine source. Treat a patch built from this the same as any other new format writer in
/// this project: build it, install it, and look.
pub fn export_double_rate_motion_patch(archive_path: &Path, entry_path: &str, bone_names: &[String], patch_dir: &Path, group: &str) -> Result<PathBuf> {
    let stage_dir = patch_dir.join("_motion_stage");
    let _ = fs::remove_dir_all(&stage_dir);
    let staged = stage_dir.join(entry_path.trim_start_matches('/'));

    let data = read_raw_entry_bytes(archive_path, entry_path)?;
    let tracks = xmot::parse_motion(&data, bone_names)?;
    let doubled = xmot::resample_double_rate(&tracks);
    let rebuilt = xmot::rebuild_motion_file(&data, bone_names, &doubled)?;
    if let Some(parent) = staged.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&staged, rebuilt).with_context(|| format!("writing {}", staged.display()))?;

    let patch_path = next_patch_path(archive_path, patch_dir, group)?;
    if let Some(parent) = patch_path.parent() {
        fs::create_dir_all(parent)?;
    }
    pak::write_archive_from_dir_with(&stage_dir, &patch_path, pak::FileCompression::ZLib)?;
    let _ = fs::remove_dir_all(&stage_dir);
    Ok(patch_path)
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

    // `map_kd`/`map_bump` can now be either a short dev-time name straight from mimicry-helper
    // (e.g. "Foo_Diffuse_01.tga", the original real case this was written for) OR a real
    // absolute path this app itself wrote in place (see `embed_real_texture_paths`, which
    // rewrites the SAME cached `.mtl` on disk so the exported `.obj` is self-sufficient in
    // other tools). The caller (this app's own frontend) always wants a bare base name to
    // match against its own library listing by name — a real regression found live: once a
    // mesh's `.mtl` got a real absolute path embedded, this function started returning that
    // whole path as `diffuse`, and the frontend's `findTextureByBaseName` (which strips only
    // the extension, not a directory) could never match a full path against a bare library
    // entry name, so the picker silently showed "not selected" for every mesh whose cache had
    // already been touched by `embed_real_texture_paths`. Always take just the file-name
    // component, regardless of which of the two `.mtl` flavors this is reading.
    let basename = |s: &str| -> String {
        s.rsplit(['/', '\\']).next().unwrap_or(s).to_string()
    };
    let diffuse = primary.and_then(|m| m.map_kd.as_deref().map(basename)).or_else(|| Some(primary_name.clone()));
    let normal = primary
        .and_then(|m| m.map_bump.as_deref().map(basename))
        .or_else(|| diffuse.as_deref().and_then(guess_normal_from_diffuse_name));

    Ok(MaterialTextureRefs { diffuse, normal })
}

fn strip_ext_lower(name: &str) -> String {
    match name.rfind('.') {
        Some(i) => name[..i].to_ascii_lowercase(),
        None => name.to_ascii_lowercase(),
    }
}

/// Rewrites the `.mtl` sibling of `obj_path` so every real material gets an explicit
/// `map_Kd`/`map_bump` line pointing at the ABSOLUTE path of its real matched texture PNG in
/// `library_out_dir` — the same base-name auto-match this app's own UI already does (see
/// `findTextureByBaseName` in `app/src/lib/library.ts`), just baked into the file this time
/// instead of only existing in this app's own runtime state.
///
/// WHY: mimicry-helper's own `.mtl` output has no map lines at all for materials that are
/// named after their texture directly (the common real case — see `read_material_texture_refs`'s
/// doc comment), which is fine for THIS app (it does its own name matching at read time) but
/// means the exported `.obj`/`.mtl` pair is not self-sufficient in any *other* real tool
/// (Blender, Rimy3D, ...) — opening it there shows an untextured/gray material because there is
/// simply no texture file path recorded anywhere in the file. Confirmed against real data: a
/// byte-exact comparison of our own archive-extracted mesh bytes vs. the same file extracted by
/// Risenaut (the original, independently-authored PAK tool) showed IDENTICAL geometry — so this
/// gap was never a data-correctness bug, only a missing interoperability feature.
///
/// Only fills in materials that don't already have an explicit map — never overwrites a real
/// mimicry-supplied `map_Kd`/`map_bump`. Returns how many map lines were added.
pub fn embed_real_texture_paths(obj_path: &Path, library_out_dir: &Path) -> Result<usize> {
    let mtl_path = obj_path.with_extension("mtl");
    let mtl_text = match fs::read_to_string(&mtl_path) {
        Ok(t) => t,
        Err(_) => return Ok(0), // no .mtl (e.g. an older cache) — nothing to do, not an error
    };

    let library = list_library(library_out_dir).unwrap_or_default();
    let find_by_exact_name = |name: &str| -> Option<String> {
        let target = strip_ext_lower(name);
        library
            .iter()
            .find(|e| strip_ext_lower(&e.name) == target)
            .map(|e| library_out_dir.join(&e.png_rel).to_string_lossy().replace('\\', "/"))
    };
    // A "_Ghost" (spectral/translucent item variant) material has no texture of its own — the
    // real game tints the BASE texture via a material property at runtime instead of baking a
    // separate image (confirmed on real data: "Ani_Hero_Helmet_Titanlord_01_Diffuse_S1_Ghost"
    // has no matching library entry, but the non-Ghost "...Diffuse_S1" does). Fall back to the
    // base name before giving up, so Ghost variants get the same real texture the base item
    // has instead of silently rendering untextured.
    let find_real_path = |ref_name: &str| -> Option<String> {
        find_by_exact_name(ref_name).or_else(|| ref_name.strip_suffix("_Ghost").and_then(find_by_exact_name))
    };

    let mut out = String::with_capacity(mtl_text.len());
    let mut current_name: Option<String> = None;
    let mut current_has_map_kd = false;
    let mut current_has_map_bump = false;
    let mut added = 0usize;

    // Emits the pending map_Kd/map_bump lines (if resolvable and not already declared) for the
    // material we just finished reading, right before moving on to the next `newmtl`/EOF.
    let flush_pending = |out: &mut String, name: &Option<String>, has_kd: bool, has_bump: bool, added: &mut usize| {
        let Some(name) = name else { return };
        if !has_kd {
            if let Some(path) = find_real_path(name) {
                out.push_str(&format!("map_Kd {path}\r\n"));
                *added += 1;
                if !has_bump {
                    if let Some(normal_name) = guess_normal_from_diffuse_name(name).and_then(|n| find_real_path(&n)) {
                        out.push_str(&format!("map_bump {normal_name}\r\n"));
                    }
                }
            }
        }
    };

    for line in mtl_text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("newmtl ") {
            flush_pending(&mut out, &current_name, current_has_map_kd, current_has_map_bump, &mut added);
            current_name = Some(name.trim().to_string());
            current_has_map_kd = false;
            current_has_map_bump = false;
        } else if trimmed.starts_with("map_Kd ") {
            current_has_map_kd = true;
        } else if trimmed.starts_with("map_bump ") {
            current_has_map_bump = true;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    flush_pending(&mut out, &current_name, current_has_map_kd, current_has_map_bump, &mut added);

    if added > 0 {
        fs::write(&mtl_path, out)?;
    }
    Ok(added)
}

/// Which enhancement engine `regenerate` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegenEngine {
    /// Real AI (Replicate) when an API key is configured AND the texture is photo-like;
    /// silently falls back to Lanczos otherwise — the "paste a key and it upgrades itself"
    /// behavior the UI relies on.
    #[default]
    Auto,
    /// Local Lanczos3 upscale only (today's baseline; also the only correct choice for
    /// normal/specular data maps regardless of configuration).
    Lanczos,
    /// Force the AI path; errors out loudly when no key is configured or the call fails.
    Ai,
}

impl std::str::FromStr for RegenEngine {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "lanczos" => Ok(Self::Lanczos),
            "ai" => Ok(Self::Ai),
            other => anyhow::bail!("unknown engine '{other}' (expected auto|lanczos|ai)"),
        }
    }
}

/// Regenerates an already-extracted PNG into the `edited/` sibling directory, ready for
/// review/`apply`. Engine selection (see `RegenEngine`): with a configured API key
/// (settings.json `aiApiKey` / env `RISENLAB_AI_KEY`) photo-like textures go through the
/// real AI enhancer (`ai::enhance_png`); normal/specular data maps and unconfigured installs
/// use the local Lanczos3 upscale that was this function's original behavior.
pub fn regenerate(out_dir: &Path, png_rel: &str, scale: u32, engine: RegenEngine) -> Result<PathBuf> {
    let src = out_dir.join(png_rel);
    let dest = out_dir.join("edited").join(png_rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // scale 0 = smart auto: small textures (the game is full of 64–256px item/detail maps)
    // gain the most from a 4x upscale, while 512+ atlases double to a comfortable 1–2k
    // without exploding VRAM/patch size.
    let scale = if scale == 0 {
        let (w, h) = image::image_dimensions(&src).with_context(|| format!("reading dimensions of {}", src.display()))?;
        if w.max(h) <= 256 {
            4
        } else {
            2
        }
    } else {
        scale
    };

    let want_ai = match engine {
        RegenEngine::Lanczos => false,
        // AI models are trained on photos; normal/specular maps encode vectors, not colors —
        // they must stay on the faithful local path even when forced (see `ai::is_data_map`).
        RegenEngine::Ai | RegenEngine::Auto => !crate::ai::is_data_map(png_rel),
    };
    if want_ai {
        match crate::ai::load_config() {
            Some(cfg) => match crate::ai::enhance_png(&cfg, &src, png_rel, scale) {
                Ok(bytes) => {
                    // Provider may answer in png/jpeg/webp — the pipeline stays PNG.
                    let img = image::load_from_memory(&bytes).context("decoding AI-enhanced image")?;
                    img.save(&dest).with_context(|| format!("writing {}", dest.display()))?;
                    return Ok(dest);
                }
                Err(e) if engine == RegenEngine::Ai => return Err(e),
                Err(e) => {
                    eprintln!("AI enhancement failed for {png_rel} ({e:#}); falling back to Lanczos");
                }
            },
            None if engine == RegenEngine::Ai => {
                anyhow::bail!(
                    "AI engine requested but no API key is configured (settings.json aiApiKey or RISENLAB_AI_KEY)"
                )
            }
            None => {}
        }
    }

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
    fn read_material_texture_refs_returns_a_bare_name_even_when_map_kd_is_a_real_absolute_path() {
        // Real regression found live: once `embed_real_texture_paths` rewrites a cached .mtl
        // with a real ABSOLUTE path (so the exported .obj is self-sufficient in other tools —
        // see that function's own doc comment), this function started returning that whole
        // path as `diffuse` — and the frontend's own `findTextureByBaseName` (which only strips
        // an extension, not a directory) could never match a full path against a bare library
        // entry name, so the picker silently showed "not selected" for every mesh whose cache
        // had already been touched once. Must always come back as a bare file name, regardless
        // of which of the two real `.mtl` shapes (short dev-time name vs. embedded real path)
        // this is reading.
        let out_dir = temp_dir("material_refs_absolute_path");
        let obj_path = out_dir.join("apple.obj");
        fs::write(
            out_dir.join("apple.mtl"),
            "newmtl ItMisc_01_Diffuse_01\r\nmap_Kd C:/Users/rusak/Desktop/RisenLab-Textures/compiled/images/Special/ItMisc_01_Diffuse_01.png\r\nmap_bump C:/Users/rusak/Desktop/RisenLab-Textures/compiled/images/Special/ItMisc_01_Normal_01.png\r\n\r\n",
        )
        .unwrap();

        let refs = read_material_texture_refs(&obj_path).unwrap();
        assert_eq!(refs.diffuse.as_deref(), Some("ItMisc_01_Diffuse_01.png"));
        assert_eq!(refs.normal.as_deref(), Some("ItMisc_01_Normal_01.png"));

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
    fn embed_real_texture_paths_adds_real_absolute_map_lines_for_name_only_materials() {
        let out_dir = temp_dir("embed_paths");
        let library_dir = temp_dir("embed_paths_library");

        // A real texture library, same shape `extract_all` produces: a manifest.tsv plus the
        // real PNG files it points at.
        let mut manifest = fs::File::create(library_dir.join(MANIFEST_NAME)).unwrap();
        for (entry_path, png_rel) in [
            ("/Special/ItWpn_Axes_01_Diffuse_01._ximg", "compiled/images/Special/ItWpn_Axes_01_Diffuse_01.png"),
            ("/Special/ItWpn_Axes_01_Normal_01._ximg", "compiled/images/Special/ItWpn_Axes_01_Normal_01.png"),
        ] {
            write_manifest_line(
                &mut manifest,
                &ManifestEntry {
                    archive: PathBuf::from("C:/Game/data/compiled/images.pak"),
                    group: "compiled".to_string(),
                    entry_path: entry_path.to_string(),
                    png_rel: png_rel.to_string(),
                    hash: 0,
                },
            )
            .unwrap();
            let png_path = library_dir.join(png_rel);
            fs::create_dir_all(png_path.parent().unwrap()).unwrap();
            fs::write(&png_path, b"fake png bytes").unwrap();
        }
        drop(manifest);

        // A real-shaped .mtl (mimicry's own output): no explicit map lines, material named
        // directly after its texture — see `read_material_texture_refs`'s doc comment.
        let obj_path = out_dir.join("axe.obj");
        fs::write(out_dir.join("axe.mtl"), "newmtl ItWpn_Axes_01_Diffuse_01\r\n\r\n").unwrap();

        let added = embed_real_texture_paths(&obj_path, &library_dir).unwrap();
        assert_eq!(added, 1);

        let mtl_text = fs::read_to_string(out_dir.join("axe.mtl")).unwrap();
        let expected_diffuse = library_dir.join("compiled/images/Special/ItWpn_Axes_01_Diffuse_01.png");
        let expected_normal = library_dir.join("compiled/images/Special/ItWpn_Axes_01_Normal_01.png");
        assert!(
            mtl_text.contains(&format!("map_Kd {}", expected_diffuse.to_string_lossy().replace('\\', "/"))),
            "expected a real map_Kd line, got:\n{mtl_text}"
        );
        assert!(
            mtl_text.contains(&format!("map_bump {}", expected_normal.to_string_lossy().replace('\\', "/"))),
            "expected a real map_bump line, got:\n{mtl_text}"
        );

        // Idempotent: re-running once the paths are already there adds nothing more and
        // doesn't duplicate the lines.
        let added_again = embed_real_texture_paths(&obj_path, &library_dir).unwrap();
        assert_eq!(added_again, 0);
        let mtl_text_again = fs::read_to_string(out_dir.join("axe.mtl")).unwrap();
        assert_eq!(mtl_text_again.matches("map_Kd").count(), 1);

        fs::remove_dir_all(&out_dir).ok();
        fs::remove_dir_all(&library_dir).ok();
    }

    #[test]
    fn embed_real_texture_paths_never_overwrites_an_explicit_map_kd_mimicry_already_wrote() {
        let out_dir = temp_dir("embed_paths_preserve");
        let library_dir = temp_dir("embed_paths_preserve_library");
        fs::create_dir_all(&library_dir).unwrap();
        fs::write(library_dir.join(MANIFEST_NAME), "").unwrap();

        let obj_path = out_dir.join("sword.obj");
        fs::write(
            out_dir.join("sword.mtl"),
            "newmtl Blade\r\nmap_Kd Blade_Diffuse_01.tga\r\n\r\n",
        )
        .unwrap();

        let added = embed_real_texture_paths(&obj_path, &library_dir).unwrap();
        assert_eq!(added, 0, "a material with its own real map_Kd must be left alone");
        let mtl_text = fs::read_to_string(out_dir.join("sword.mtl")).unwrap();
        assert_eq!(mtl_text.matches("map_Kd").count(), 1);
        assert!(mtl_text.contains("map_Kd Blade_Diffuse_01.tga"));

        fs::remove_dir_all(&out_dir).ok();
        fs::remove_dir_all(&library_dir).ok();
    }

    #[test]
    fn embed_real_texture_paths_falls_back_to_the_base_texture_for_a_ghost_variant() {
        // Real bug found live: "It_Helmet_TitanLord_Ghost" rendered blank/white because its
        // material is named "..._Diffuse_S1_Ghost" and no texture file has that exact name —
        // only the non-Ghost base item's texture exists. Ghost variants are a real, tinted-at-
        // runtime reuse of the base texture, not a missing asset.
        let out_dir = temp_dir("embed_paths_ghost");
        let library_dir = temp_dir("embed_paths_ghost_library");

        let mut manifest = fs::File::create(library_dir.join(MANIFEST_NAME)).unwrap();
        write_manifest_line(
            &mut manifest,
            &ManifestEntry {
                archive: PathBuf::from("C:/Game/data/compiled/images.pak"),
                group: "compiled".to_string(),
                entry_path: "/Animation/Heads/Ani_Hero_Helmet_Titanlord_01_Diffuse_S1._ximg".to_string(),
                png_rel: "compiled/images/Animation/Heads/Ani_Hero_Helmet_Titanlord_01_Diffuse_S1.png".to_string(),
                hash: 0,
            },
        )
        .unwrap();
        drop(manifest);
        let png_path = library_dir.join("compiled/images/Animation/Heads/Ani_Hero_Helmet_Titanlord_01_Diffuse_S1.png");
        fs::create_dir_all(png_path.parent().unwrap()).unwrap();
        fs::write(&png_path, b"fake png bytes").unwrap();

        let obj_path = out_dir.join("helmet_ghost.obj");
        fs::write(
            out_dir.join("helmet_ghost.mtl"),
            "newmtl Ani_Hero_Helmet_Titanlord_01_Diffuse_S1_Ghost\r\n\r\n",
        )
        .unwrap();

        let added = embed_real_texture_paths(&obj_path, &library_dir).unwrap();
        assert_eq!(added, 1, "should fall back to the base (non-Ghost) texture");
        let mtl_text = fs::read_to_string(out_dir.join("helmet_ghost.mtl")).unwrap();
        assert!(
            mtl_text.contains("map_Kd") && mtl_text.contains("Ani_Hero_Helmet_Titanlord_01_Diffuse_S1.png"),
            "expected the base texture's real path, got:\n{mtl_text}"
        );

        fs::remove_dir_all(&out_dir).ok();
        fs::remove_dir_all(&library_dir).ok();
    }

    #[test]
    fn regenerate_upscales_a_png_by_the_requested_scale_into_edited_dir() {
        let out_dir = temp_dir("regenerate");
        let png_rel = "compiled/images/Level/rock.png";
        let src = out_dir.join(png_rel);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        let img = image::RgbaImage::from_pixel(8, 4, image::Rgba([10, 20, 30, 255]));
        img.save(&src).unwrap();

        let dest = regenerate(&out_dir, png_rel, 2, RegenEngine::Lanczos).unwrap();
        assert_eq!(dest, out_dir.join("edited").join(png_rel));
        let decoded = image::ImageReader::open(&dest).unwrap().decode().unwrap();
        assert_eq!(decoded.width(), 16);
        assert_eq!(decoded.height(), 8);

        fs::remove_dir_all(&out_dir).ok();
    }

    /// scale 0 = smart auto: small textures (≤256px, the game's many item/detail maps) get
    /// 4x — they gain the most; larger atlases get 2x to keep patch/VRAM size sane.
    #[test]
    fn regenerate_scale_zero_is_smart_auto_4x_small_2x_large() {
        let out_dir = temp_dir("regenerate_scale0");
        let small_rel = "small.png";
        image::RgbaImage::from_pixel(64, 32, image::Rgba([1, 2, 3, 255]))
            .save(out_dir.join(small_rel))
            .unwrap();
        let large_rel = "large.png";
        image::RgbaImage::from_pixel(300, 100, image::Rgba([1, 2, 3, 255]))
            .save(out_dir.join(large_rel))
            .unwrap();

        let small = image::ImageReader::open(regenerate(&out_dir, small_rel, 0, RegenEngine::Lanczos).unwrap())
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!((small.width(), small.height()), (256, 128), "≤256px → 4x");
        let large = image::ImageReader::open(regenerate(&out_dir, large_rel, 0, RegenEngine::Lanczos).unwrap())
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!((large.width(), large.height()), (600, 200), ">256px → 2x");

        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn regenerate_errors_when_source_png_is_missing() {
        let out_dir = temp_dir("regenerate_missing");
        assert!(regenerate(&out_dir, "nope.png", 2, RegenEngine::Lanczos).is_err());
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
