//! Pure(ish) logic behind the Tauri commands in `main.rs`, kept in its own module so every
//! piece of it can be unit tested without spinning up a Tauri `App`/webview.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use risenlab::{gamepath, pak, ximg};

// ---------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub game_exe: Option<String>,
    pub output_dir: String,
    pub patch_dir: String,
    pub review_html: String,
    pub language: String,
    /// Replicate API token for real AI texture enhancement (see `risenlab::ai`). `None` /
    /// empty = feature dormant, local Lanczos is used. `#[serde(default)]` keeps old
    /// settings.json files (without these keys) loading.
    /// "replicate" (default) | "stability".
    #[serde(default)]
    pub ai_provider: Option<String>,
    #[serde(default)]
    pub ai_api_key: Option<String>,
    /// Replicate model override (`owner/name`); default is `risenlab::ai::DEFAULT_MODEL`.
    #[serde(default)]
    pub ai_model: Option<String>,
    /// 0.1–0.9 "how much may the AI invent" dial (see `risenlab::ai::AiConfig::creativity`).
    #[serde(default)]
    pub ai_creativity: Option<f32>,
}

/// Reads `USERPROFILE` (Windows home dir); falls back to `.` if unset, which only happens
/// in unusual/non-Windows environments — not worth failing startup over.
pub fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Default output/patch/review paths mirror the ones already shown in the approved design
/// (`SettingsScreen.dc.html`'s `pathRows`) — same naming, just rooted at the real user's
/// Desktop instead of the mockup's placeholder `Гравець`/`Player` user.
pub fn default_settings_for(home: &Path) -> AppSettings {
    let desktop = home.join("Desktop");
    AppSettings {
        game_exe: None,
        output_dir: desktop.join("RisenLab-Textures").to_string_lossy().into_owned(),
        patch_dir: desktop.join("RisenLab-Patch").to_string_lossy().into_owned(),
        review_html: desktop.join("RisenLab-Review.html").to_string_lossy().into_owned(),
        language: "uk".to_string(),
        ai_provider: None,
        ai_api_key: None,
        ai_model: None,
        ai_creativity: None,
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

/// Loads settings from disk; any failure (missing file, corrupt JSON) falls back to
/// `fallback` rather than blocking startup — settings are recoverable, a crash on launch
/// isn't worth it for a malformed local file.
pub fn load_settings(path: &Path, fallback: AppSettings) -> AppSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(fallback)
}

pub fn save_settings_to(path: &Path, settings: &AppSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, json)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Review status sidecar (approve/reject queue)
// ---------------------------------------------------------------------------------------

pub type ReviewStatusMap = HashMap<String, String>;

pub fn review_status_path(output_dir: &Path) -> PathBuf {
    output_dir.join("review_status.json")
}

pub fn load_review_status(path: &Path) -> ReviewStatusMap {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_review_status(path: &Path, map: &ReviewStatusMap) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(map)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Recursively lists every `.png` under `output_dir/edited`, as posix-style paths relative
/// to `output_dir` (i.e. `png_rel` values, same shape `batch::list_library` uses) — these are
/// exactly the textures that have a pending AI-regenerated variant to review.
pub fn list_edited_pngs(output_dir: &Path) -> Result<Vec<String>> {
    let edited_root = output_dir.join("edited");
    let mut out = Vec::new();
    if edited_root.is_dir() {
        walk_pngs(&edited_root, &edited_root, &mut out)?;
    }
    out.sort();
    Ok(out
        .into_iter()
        .map(|p| format!("{}", p.replace('\\', "/")))
        .collect())
}

fn walk_pngs(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_pngs(root, &path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("png") {
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            out.push(rel);
        }
    }
    Ok(())
}

/// Pairs every edited png with its current status, defaulting to `"pending"` for anything
/// not yet approved/rejected in the sidecar map.
pub fn review_queue_from(edited: &[String], status: &ReviewStatusMap) -> Vec<(String, String)> {
    edited
        .iter()
        .map(|png_rel| {
            let s = status.get(png_rel).cloned().unwrap_or_else(|| "pending".to_string());
            (png_rel.clone(), s)
        })
        .collect()
}

pub fn approved_pngs(status: &ReviewStatusMap) -> Vec<String> {
    let mut out: Vec<String> = status
        .iter()
        .filter(|(_, v)| v.as_str() == "approved")
        .map(|(k, _)| k.clone())
        .collect();
    out.sort();
    out
}

/// Copies every `approved` png from `edited_dir` into `staging_dir`, preserving its relative
/// path — the staging dir is then handed to `batch::apply` as its `edited_dir`, so only
/// approved textures end up in the built `.pXX` patch.
pub fn stage_approved_for_apply(edited_dir: &Path, staging_dir: &Path, approved: &[String]) -> Result<()> {
    for png_rel in approved {
        let src = edited_dir.join(png_rel);
        if !src.exists() {
            continue;
        }
        let dest = staging_dir.join(png_rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, &dest).with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Thumbnails as data: URLs (avoids configuring the Tauri asset-protocol scope)
// ---------------------------------------------------------------------------------------

const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
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

pub fn data_url_png(bytes: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64_encode(bytes))
}

// ---------------------------------------------------------------------------------------
// Game discovery
// ---------------------------------------------------------------------------------------

pub struct GameDiscovery {
    pub root: PathBuf,
    pub archives: Vec<gamepath::DiscoveredArchive>,
}

/// Composition of `gamepath`'s three steps (already unit-tested in the core crate) — kept as
/// its own function so `check_game`'s Tauri command stays a thin orchestrator.
pub fn discover_game(exe_or_shortcut: &Path) -> Result<GameDiscovery> {
    let exe = gamepath::resolve_shortcut(exe_or_shortcut)?;
    let root = gamepath::discover_game_root(&exe)
        .ok_or_else(|| anyhow!("could not find a data/ folder with archives above {}", exe.display()))?;
    let archives = gamepath::discover_archives(&root)?;
    Ok(GameDiscovery { root, archives })
}

pub fn sum_file_sizes(paths: &[PathBuf]) -> u64 {
    paths
        .iter()
        .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum()
}

/// Total size in bytes of every file under `dir`, recursively. Missing `dir` (nothing
/// extracted/generated yet) is `0`, not an error — mirrors the dev-bridge's `dirSizeBytes`,
/// used by the Dashboard's "Textures" disk-usage tile.
pub fn dir_size_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += dir_size_bytes(&path);
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}

// ---------------------------------------------------------------------------------------
// Texture metadata (detail panel)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureMeta {
    pub width: i32,
    pub height: i32,
    pub pixel_format: String,
    pub file_size: u64,
}

pub fn read_texture_meta(archive_path: &Path, entry_path: &str) -> Result<TextureMeta> {
    let mut archive = pak::PakArchive::open(archive_path)
        .with_context(|| format!("opening {}", archive_path.display()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "risenlab_ui_logic_test_{tag}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // --- settings ---

    #[test]
    fn default_settings_root_desktop_paths_match_the_approved_design_defaults() {
        let s = default_settings_for(Path::new("C:/Users/Гравець"));
        assert!(s.output_dir.ends_with("RisenLab-Textures"));
        assert!(s.patch_dir.ends_with("RisenLab-Patch"));
        assert!(s.review_html.ends_with("RisenLab-Review.html"));
        assert_eq!(s.language, "uk");
        assert_eq!(s.game_exe, None);
    }

    #[test]
    fn settings_path_joins_config_dir() {
        assert_eq!(settings_path(Path::new("C:/cfg")), PathBuf::from("C:/cfg/settings.json"));
    }

    #[test]
    fn load_settings_returns_fallback_when_file_missing() {
        let dir = temp_dir("settings_missing");
        let fallback = default_settings_for(&dir);
        let loaded = load_settings(&dir.join("nope.json"), fallback.clone());
        assert_eq!(loaded, fallback);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_settings_returns_fallback_when_file_is_corrupt() {
        let dir = temp_dir("settings_corrupt");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();
        let fallback = default_settings_for(&dir);
        let loaded = load_settings(&path, fallback.clone());
        assert_eq!(loaded, fallback);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_then_load_settings_round_trips() {
        let dir = temp_dir("settings_roundtrip");
        let path = settings_path(&dir);
        let mut settings = default_settings_for(&dir);
        settings.game_exe = Some("C:/Games/Risen/Risen.exe".to_string());
        settings.language = "en".to_string();
        save_settings_to(&path, &settings).unwrap();
        let loaded = load_settings(&path, default_settings_for(&dir));
        assert_eq!(loaded, settings);
        std::fs::remove_dir_all(&dir).ok();
    }

    // --- review status ---

    #[test]
    fn review_status_path_joins_output_dir() {
        assert_eq!(
            review_status_path(Path::new("C:/out")),
            PathBuf::from("C:/out/review_status.json")
        );
    }

    #[test]
    fn load_review_status_defaults_to_empty_when_missing_or_corrupt() {
        let dir = temp_dir("review_status_missing");
        assert!(load_review_status(&dir.join("nope.json")).is_empty());
        let corrupt = dir.join("bad.json");
        std::fs::write(&corrupt, "not json").unwrap();
        assert!(load_review_status(&corrupt).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_then_load_review_status_round_trips() {
        let dir = temp_dir("review_status_roundtrip");
        let path = review_status_path(&dir);
        let mut map = ReviewStatusMap::new();
        map.insert("a/b.png".to_string(), "approved".to_string());
        map.insert("c/d.png".to_string(), "rejected".to_string());
        save_review_status(&path, &map).unwrap();
        assert_eq!(load_review_status(&path), map);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_edited_pngs_finds_nested_pngs_and_ignores_other_files() {
        let dir = temp_dir("list_edited");
        let edited = dir.join("edited");
        std::fs::create_dir_all(edited.join("compiled/images/Level")).unwrap();
        std::fs::write(edited.join("compiled/images/Level/rock.png"), b"x").unwrap();
        std::fs::write(edited.join("compiled/images/Level/rock.txt"), b"x").unwrap();
        std::fs::write(edited.join("compiled/images/other.png"), b"x").unwrap();

        let mut found = list_edited_pngs(&dir).unwrap();
        found.sort();
        assert_eq!(
            found,
            vec![
                "compiled/images/Level/rock.png".to_string(),
                "compiled/images/other.png".to_string(),
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_edited_pngs_returns_empty_when_no_edited_dir_yet() {
        let dir = temp_dir("list_edited_none");
        assert!(list_edited_pngs(&dir).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn review_queue_from_defaults_missing_entries_to_pending() {
        let mut status = ReviewStatusMap::new();
        status.insert("a.png".to_string(), "approved".to_string());
        let queue = review_queue_from(&["a.png".to_string(), "b.png".to_string()], &status);
        assert_eq!(
            queue,
            vec![
                ("a.png".to_string(), "approved".to_string()),
                ("b.png".to_string(), "pending".to_string()),
            ]
        );
    }

    #[test]
    fn approved_pngs_filters_and_sorts() {
        let mut status = ReviewStatusMap::new();
        status.insert("z.png".to_string(), "approved".to_string());
        status.insert("a.png".to_string(), "approved".to_string());
        status.insert("m.png".to_string(), "rejected".to_string());
        assert_eq!(approved_pngs(&status), vec!["a.png".to_string(), "z.png".to_string()]);
    }

    #[test]
    fn stage_approved_for_apply_copies_only_approved_and_preserves_structure() {
        let dir = temp_dir("stage_approved");
        let edited = dir.join("edited");
        let staging = dir.join("staging");
        std::fs::create_dir_all(edited.join("Level")).unwrap();
        std::fs::write(edited.join("Level/a.png"), b"approved-content").unwrap();
        std::fs::write(edited.join("Level/b.png"), b"not-approved").unwrap();

        stage_approved_for_apply(&edited, &staging, &["Level/a.png".to_string()]).unwrap();

        assert_eq!(std::fs::read(staging.join("Level/a.png")).unwrap(), b"approved-content");
        assert!(!staging.join("Level/b.png").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stage_approved_for_apply_skips_approved_entries_missing_on_disk() {
        let dir = temp_dir("stage_approved_missing");
        let edited = dir.join("edited");
        let staging = dir.join("staging");
        std::fs::create_dir_all(&edited).unwrap();
        // "approved" but the file was never actually regenerated (e.g. race/manual edit) —
        // must not error, just skip it.
        stage_approved_for_apply(&edited, &staging, &["ghost.png".to_string()]).unwrap();
        assert!(!staging.join("ghost.png").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    // --- base64 / data urls ---

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn data_url_png_wraps_base64_with_the_right_prefix() {
        assert_eq!(data_url_png(b"foobar"), "data:image/png;base64,Zm9vYmFy");
    }

    // --- game discovery ---

    #[test]
    fn discover_game_finds_root_and_archives_from_a_synthetic_install() {
        let tmp = temp_dir("discover_game");
        let bin_dir = tmp.join("bin");
        let compiled_dir = tmp.join("data").join("compiled");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::create_dir_all(&compiled_dir).unwrap();
        std::fs::write(compiled_dir.join("images.pak"), b"12345").unwrap();
        let exe = bin_dir.join("Risen.exe");
        std::fs::write(&exe, b"x").unwrap();

        let discovery = discover_game(&exe).unwrap();
        assert_eq!(discovery.root, tmp);
        assert_eq!(discovery.archives.len(), 1);
        assert_eq!(sum_file_sizes(&discovery.archives.iter().map(|a| a.path.clone()).collect::<Vec<_>>()), 5);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn discover_game_errors_when_no_data_folder_exists() {
        let tmp = temp_dir("discover_game_missing");
        let exe = tmp.join("Risen.exe");
        std::fs::write(&exe, b"x").unwrap();
        assert!(discover_game(&exe).is_err());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn sum_file_sizes_treats_missing_files_as_zero() {
        let dir = temp_dir("sum_sizes");
        let a = dir.join("a.bin");
        std::fs::write(&a, [0u8; 10]).unwrap();
        let missing = dir.join("nope.bin");
        assert_eq!(sum_file_sizes(&[a, missing]), 10);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dir_size_bytes_sums_nested_files_and_ignores_missing_dir() {
        let dir = temp_dir("dir_size");
        std::fs::create_dir_all(dir.join("edited/Level")).unwrap();
        std::fs::write(dir.join("a.png"), [0u8; 5]).unwrap();
        std::fs::write(dir.join("edited/Level/b.png"), [0u8; 7]).unwrap();
        assert_eq!(dir_size_bytes(&dir), 12);
        assert_eq!(dir_size_bytes(&dir.join("nope")), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    // --- texture metadata ---

    fn push_prop(buf: &mut Vec<u8>, name: &str, type_name: &str, value: &[u8]) {
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&(type_name.len() as u16).to_le_bytes());
        buf.extend_from_slice(type_name.as_bytes());
        buf.extend_from_slice(&30u16.to_le_bytes());
        buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
        buf.extend_from_slice(value);
    }

    /// Same synthetic `._ximg` layout `ximg.rs`'s own tests use (see that module) — duplicated
    /// here (rather than exported cross-crate) since it's test-only fixture code.
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

    #[test]
    fn read_texture_meta_reads_real_width_height_format_from_a_packed_archive() {
        let dir = temp_dir("texture_meta");
        let src_dir = dir.join("src");
        std::fs::create_dir_all(src_dir.join("Level")).unwrap();
        let ximg_bytes = synthetic_ximg(64, 32, "DXT3");
        std::fs::write(src_dir.join("Level").join("Test._ximg"), &ximg_bytes).unwrap();

        let archive_path = dir.join("images.pak");
        pak::write_archive_from_dir(&src_dir, &archive_path).unwrap();

        let mut archive = pak::PakArchive::open(&archive_path).unwrap();
        let entry_path = archive.files()[0].path.clone();

        let meta = read_texture_meta(&archive_path, &entry_path).unwrap();
        assert_eq!(meta.width, 64);
        assert_eq!(meta.height, 32);
        assert_eq!(meta.pixel_format, "DXT3");
        assert_eq!(meta.file_size, ximg_bytes.len() as u64);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_texture_meta_errors_when_entry_not_found() {
        let dir = temp_dir("texture_meta_missing_entry");
        let src_dir = dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("real.txt"), b"x").unwrap();
        let archive_path = dir.join("archive.pak");
        pak::write_archive_from_dir(&src_dir, &archive_path).unwrap();

        assert!(read_texture_meta(&archive_path, "does/not/exist._ximg").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_texture_meta_errors_when_archive_does_not_exist() {
        let dir = temp_dir("texture_meta_missing_archive");
        assert!(read_texture_meta(&dir.join("nope.pak"), "x").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
