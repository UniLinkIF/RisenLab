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

use crate::{dds, gamepath, pak, ximg};

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
}
