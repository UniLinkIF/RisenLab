#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri shell for RisenLab. Every non-trivial piece of logic lives in a plain function
//! (below, in the `logic` module) with its own unit test — `#[tauri::command]` wrappers are
//! kept as thin as possible (open state, call a logic function, map errors to `String`)
//! since they need a running Tauri app to exercise directly.

mod logic;
mod remote;

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use logic::AppSettings;
use remote::RemoteState;
use risenlab::batch;

struct AppState {
    settings: Mutex<AppSettings>,
    settings_path: PathBuf,
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command(rename_all = "camelCase")]
fn save_settings(state: State<AppState>, settings: AppSettings) -> Result<(), String> {
    logic::save_settings_to(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
fn pick_game_path() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Risen.exe or shortcut", &["exe", "lnk"])
        .pick_file()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Was missing entirely: the frontend's `pickFolder()` never branched on `isTauri()` (unlike
/// `pickGamePath`) and always called the dev-bridge's `/api/pick-folder` HTTP route — which
/// doesn't exist in the packaged app at all (no dev server, and the local Tauri UI doesn't talk
/// to the remote-access HTTP server either). Every "Огляд…" button next to the output/patch/
/// review-html paths in Settings was silently broken in the packaged .exe. Real bug, found while
/// building the texture export/import feature below (owner report, 2026-07-20: "проєкт має
/// бути готовим... я хочу могти вже ділитись").
#[tauri::command]
fn pick_folder() -> Option<String> {
    rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().into_owned())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameCheckResult {
    root: String,
    archive_count: usize,
    total_bytes: u64,
    textures_extracted: usize,
}

#[tauri::command]
async fn check_game(state: State<'_, AppState>) -> Result<GameCheckResult, String> {
    let (exe, out_dir) = {
        let s = state.settings.lock().unwrap();
        let exe = s
            .game_exe
            .clone()
            .ok_or_else(|| "Спершу вкажіть шлях до гри".to_string())?;
        (PathBuf::from(exe), PathBuf::from(s.output_dir.clone()))
    };

    let discovery = logic::discover_game(&exe).map_err(|e| e.to_string())?;
    let total_bytes = logic::sum_file_sizes(
        &discovery
            .archives
            .iter()
            .map(|a| a.path.clone())
            .collect::<Vec<_>>(),
    );
    let archive_count = discovery.archives.len();

    let root_str = discovery.root.to_string_lossy().into_owned();
    let extracted = tauri::async_runtime::spawn_blocking(move || batch::extract_all(&exe, &out_dir))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    Ok(GameCheckResult {
        root: root_str,
        archive_count,
        total_bytes,
        textures_extracted: extracted,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryEntryDto {
    group: String,
    archive_path: String,
    archive_stem: String,
    entry_path: String,
    png_rel: String,
    name: String,
    folder: String,
}

impl From<batch::LibraryEntry> for LibraryEntryDto {
    fn from(e: batch::LibraryEntry) -> Self {
        Self {
            group: e.group,
            archive_path: e.archive_path,
            archive_stem: e.archive_stem,
            entry_path: e.entry_path,
            png_rel: e.png_rel,
            name: e.name,
            folder: e.folder,
        }
    }
}

#[tauri::command]
fn list_library(state: State<AppState>) -> Result<Vec<LibraryEntryDto>, String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    batch::list_library(&out_dir)
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn read_texture_data_url(state: State<AppState>, png_rel: String) -> Result<String, String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    let bytes = std::fs::read(out_dir.join(&png_rel)).map_err(|e| e.to_string())?;
    Ok(logic::data_url_png(&bytes))
}

#[tauri::command(rename_all = "camelCase")]
fn read_edited_data_url(state: State<AppState>, png_rel: String) -> Result<String, String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    let bytes = std::fs::read(out_dir.join("edited").join(&png_rel)).map_err(|e| e.to_string())?;
    Ok(logic::data_url_png(&bytes))
}

#[tauri::command(rename_all = "camelCase")]
fn texture_meta(archive_path: String, entry_path: String) -> Result<logic::TextureMeta, String> {
    logic::read_texture_meta(&PathBuf::from(archive_path), &entry_path).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MeshEntryDto {
    group: String,
    archive_path: String,
    archive_stem: String,
    entry_path: String,
    name: String,
    folder: String,
}

impl From<batch::MeshEntry> for MeshEntryDto {
    fn from(e: batch::MeshEntry) -> Self {
        Self {
            group: e.group,
            archive_path: e.archive_path,
            archive_stem: e.archive_stem,
            entry_path: e.entry_path,
            name: e.name,
            folder: e.folder,
        }
    }
}

#[tauri::command]
fn list_meshes(state: State<AppState>) -> Result<Vec<MeshEntryDto>, String> {
    let exe = state
        .settings
        .lock()
        .unwrap()
        .game_exe
        .clone()
        .ok_or_else(|| "Спершу вкажіть шлях до гри".to_string())?;
    batch::list_meshes(&PathBuf::from(exe))
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn mesh_to_obj(state: State<AppState>, archive_path: String, entry_path: String) -> Result<String, String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    let mesh_cache_dir = out_dir
        .parent()
        .map(|p| p.join("meshes"))
        .unwrap_or_else(|| out_dir.join("meshes"));
    let obj_path = batch::mesh_to_obj_from_archive(&PathBuf::from(archive_path), &entry_path, &mesh_cache_dir)
        .map_err(|e| e.to_string())?;
    // Best-effort, mirrors the dev-server's /api/mesh-obj — makes the exported .obj
    // self-sufficient (real map_Kd/map_bump paths) in any real 3D tool, not just this app.
    let _ = batch::embed_real_texture_paths(&obj_path, &out_dir);
    std::fs::read_to_string(&obj_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_actors(state: State<AppState>) -> Result<Vec<MeshEntryDto>, String> {
    let exe = state
        .settings
        .lock()
        .unwrap()
        .game_exe
        .clone()
        .ok_or_else(|| "Спершу вкажіть шлях до гри".to_string())?;
    batch::list_actors(&PathBuf::from(exe))
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn actor_to_obj(state: State<AppState>, archive_path: String, entry_path: String) -> Result<String, String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    let actor_cache_dir = out_dir
        .parent()
        .map(|p| p.join("actors"))
        .unwrap_or_else(|| out_dir.join("actors"));
    let obj_path = batch::actor_to_obj_from_archive(&PathBuf::from(archive_path), &entry_path, &actor_cache_dir)
        .map_err(|e| e.to_string())?;
    // See the matching comment in mesh_to_obj.
    let _ = batch::embed_real_texture_paths(&obj_path, &out_dir);
    std::fs::read_to_string(&obj_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_motions(state: State<AppState>) -> Result<Vec<MeshEntryDto>, String> {
    let exe = state
        .settings
        .lock()
        .unwrap()
        .game_exe
        .clone()
        .ok_or_else(|| "Спершу вкажіть шлях до гри".to_string())?;
    batch::list_motions(&PathBuf::from(exe))
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn mesh_texture_refs(
    state: State<AppState>,
    archive_path: String,
    entry_path: String,
    kind: String,
) -> Result<batch::MaterialTextureRefs, String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    let obj_path = if kind == "actor" {
        let cache_dir = out_dir.parent().map(|p| p.join("actors")).unwrap_or_else(|| out_dir.join("actors"));
        batch::actor_to_obj_from_archive(&PathBuf::from(archive_path), &entry_path, &cache_dir)
    } else {
        let cache_dir = out_dir.parent().map(|p| p.join("meshes")).unwrap_or_else(|| out_dir.join("meshes"));
        batch::mesh_to_obj_from_archive(&PathBuf::from(archive_path), &entry_path, &cache_dir)
    }
    .map_err(|e| e.to_string())?;
    batch::read_material_texture_refs(&obj_path).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn actor_skeleton(archive_path: String, entry_path: String) -> Result<Vec<risenlab::xmac::SkeletonNode>, String> {
    batch::actor_skeleton(&PathBuf::from(archive_path), &entry_path).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn motion_tracks(
    archive_path: String,
    entry_path: String,
    bone_names: Vec<String>,
    smooth: Option<f32>,
    expressiveness: Option<f32>,
    secondary: Option<f32>,
    sharpness: Option<f32>,
    double_rate: Option<bool>,
) -> Result<Vec<risenlab::xmot::BoneMotion>, String> {
    batch::motion_tracks(&PathBuf::from(archive_path), &entry_path, &bone_names)
        .map(|tracks| {
            let tracks = match smooth {
                Some(s) if s > 0.0 => risenlab::xmot::smooth_tracks(&tracks, s),
                _ => tracks,
            };
            let tracks = risenlab::xmot::stylize_tracks(&tracks, expressiveness.unwrap_or(0.0), secondary.unwrap_or(0.0), sharpness.unwrap_or(0.0));
            if double_rate.unwrap_or(false) {
                risenlab::xmot::resample_double_rate(&tracks)
            } else {
                tracks
            }
        })
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn actor_skinned_mesh(archive_path: String, entry_path: String) -> Result<risenlab::xmesh_skin::SkinnedMesh, String> {
    batch::actor_skinned_mesh(&PathBuf::from(archive_path), &entry_path).map_err(|e| e.to_string())
}

/// Builds a real `risenlab::ai::AiConfig` from the packaged app's OWN settings — never from
/// `ai::load_config()`, which only ever reads `Desktop\RisenLab-Project\settings.json` (the
/// CLI/dev-bridge's own settings file, a DIFFERENT location than this app's real settings, see
/// `batch::regenerate`'s doc comment for the full story). `RISENLAB_AI_KEY` still overrides the
/// key, same as the CLI, for one-off testing without touching the saved settings.
fn resolve_ai_config(settings: &logic::AppSettings) -> Option<risenlab::ai::AiConfig> {
    let env_key = std::env::var("RISENLAB_AI_KEY").ok().filter(|k| !k.trim().is_empty());
    let key = env_key.as_deref().or(settings.ai_api_key.as_deref()).unwrap_or("");
    risenlab::ai::config_from_parts(
        settings.ai_provider.as_deref(),
        key,
        settings.ai_model.as_deref(),
        settings.ai_creativity,
        settings.ai_regenerate.unwrap_or(false),
    )
}

#[tauri::command(rename_all = "camelCase")]
fn regenerate_texture(state: State<AppState>, png_rel: String, scale: Option<u32>) -> Result<(), String> {
    let (out_dir, ai_config) = {
        let s = state.settings.lock().unwrap();
        (PathBuf::from(s.output_dir.clone()), resolve_ai_config(&s))
    };
    batch::regenerate(&out_dir, &png_rel, scale.unwrap_or(0), risenlab::batch::RegenEngine::Auto, ai_config.as_ref())
        .map_err(|e| e.to_string())?;
    // Clear any leftover approve/reject decision from a PREVIOUS regenerate (see
    // logic::reset_review_status doc comment) — otherwise this brand-new result stays invisible
    // in the Review screen, reading as "regenerate did nothing" when it actually worked fine.
    logic::reset_review_status(&logic::review_status_path(&out_dir), &png_rel).map_err(|e| e.to_string())
}

/// "Витягнути": opens a native Save dialog defaulting to the texture's own filename, then
/// copies its current variant (edited/ if reviewed, otherwise the original) there byte-for-byte
/// — ready to open in an external editor. Returns the saved path, or `None` if cancelled.
#[tauri::command(rename_all = "camelCase")]
fn export_texture(state: State<AppState>, png_rel: String) -> Result<Option<String>, String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    let file_name = PathBuf::from(&png_rel).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "texture.png".to_string());
    let Some(dest) = rfd::FileDialog::new().set_file_name(&file_name).add_filter("PNG", &["png"]).save_file() else {
        return Ok(None);
    };
    batch::export_texture_to(&out_dir, &png_rel, &dest).map_err(|e| e.to_string())?;
    Ok(Some(dest.to_string_lossy().into_owned()))
}

/// The other half: opens a native Open dialog, brings the picked image back in as the texture's
/// `edited/` variant (same slot AI output lands in — goes through the normal review queue from
/// there), and clears any stale review decision the same way a fresh AI regenerate does.
/// Returns the source path picked, or `None` if cancelled.
#[tauri::command(rename_all = "camelCase")]
fn import_edited_texture(state: State<AppState>, png_rel: String) -> Result<Option<String>, String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    let Some(src) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg", "webp", "bmp", "tga"]).pick_file() else {
        return Ok(None);
    };
    batch::import_edited_texture(&out_dir, &png_rel, &src).map_err(|e| e.to_string())?;
    logic::reset_review_status(&logic::review_status_path(&out_dir), &png_rel).map_err(|e| e.to_string())?;
    Ok(Some(src.to_string_lossy().into_owned()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewItemDto {
    png_rel: String,
    status: String,
}

#[tauri::command]
fn review_queue(state: State<AppState>) -> Result<Vec<ReviewItemDto>, String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    let edited = logic::list_edited_pngs(&out_dir).map_err(|e| e.to_string())?;
    let status = logic::load_review_status(&logic::review_status_path(&out_dir));
    Ok(logic::review_queue_from(&edited, &status)
        .into_iter()
        .map(|(png_rel, status)| ReviewItemDto { png_rel, status })
        .collect())
}

#[tauri::command(rename_all = "camelCase")]
fn set_review_status(state: State<AppState>, png_rel: String, status: String) -> Result<(), String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    let path = logic::review_status_path(&out_dir);
    let mut map = logic::load_review_status(&path);
    if status == "rejected" {
        let edited = out_dir.join("edited").join(&png_rel);
        let _ = std::fs::remove_file(edited);
        map.remove(&png_rel);
    } else {
        map.insert(png_rel, status);
    }
    logic::save_review_status(&path, &map).map_err(|e| e.to_string())
}

#[tauri::command]
fn build_patches(state: State<AppState>) -> Result<Vec<String>, String> {
    let (out_dir, patch_dir) = {
        let s = state.settings.lock().unwrap();
        (
            PathBuf::from(s.output_dir.clone()),
            PathBuf::from(s.patch_dir.clone()),
        )
    };
    let status = logic::load_review_status(&logic::review_status_path(&out_dir));
    let approved = logic::approved_pngs(&status);

    let staging = out_dir.join("_approved_stage");
    let _ = std::fs::remove_dir_all(&staging);
    logic::stage_approved_for_apply(&out_dir.join("edited"), &staging, &approved)
        .map_err(|e| e.to_string())?;

    let manifest = out_dir.join("manifest.tsv");
    let written = batch::apply(&manifest, &staging, &patch_dir).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(written
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

#[tauri::command]
fn install_patches(state: State<AppState>) -> Result<Vec<String>, String> {
    let (game_exe, patch_dir) = {
        let s = state.settings.lock().unwrap();
        (s.game_exe.clone(), PathBuf::from(s.patch_dir.clone()))
    };
    let game_exe = game_exe.ok_or("gameExe not configured")?;
    batch::install_patches(&PathBuf::from(game_exe), &patch_dir).map_err(|e| e.to_string())
}

#[tauri::command]
fn uninstall_patches(state: State<AppState>) -> Result<Vec<String>, String> {
    let (game_exe, patch_dir) = {
        let s = state.settings.lock().unwrap();
        (s.game_exe.clone(), PathBuf::from(s.patch_dir.clone()))
    };
    let game_exe = game_exe.ok_or("gameExe not configured")?;
    batch::uninstall_patches(&PathBuf::from(game_exe), &patch_dir).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn export_motion_patch(
    state: State<AppState>,
    archive_path: String,
    entry_path: String,
    bone_names: Vec<String>,
    smooth: f32,
    expressiveness: Option<f32>,
    secondary: Option<f32>,
    sharpness: Option<f32>,
) -> Result<String, String> {
    let patch_dir = PathBuf::from(state.settings.lock().unwrap().patch_dir.clone());
    let style = batch::MotionStyle {
        smooth,
        expressiveness: expressiveness.unwrap_or(0.0),
        secondary: secondary.unwrap_or(0.0),
        sharpness: sharpness.unwrap_or(0.0),
    };
    batch::export_motion_patch(&PathBuf::from(archive_path), &entry_path, &bone_names, style, &patch_dir, "compiled")
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn export_double_rate_motion_patch(state: State<AppState>, archive_path: String, entry_path: String, bone_names: Vec<String>) -> Result<String, String> {
    let patch_dir = PathBuf::from(state.settings.lock().unwrap().patch_dir.clone());
    batch::export_double_rate_motion_patch(&PathBuf::from(archive_path), &entry_path, &bone_names, &patch_dir, "compiled")
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct MotionPatchBatchResult {
    patch: String,
    failed: Vec<String>,
}

#[tauri::command(rename_all = "camelCase")]
fn export_motion_patch_batch(
    state: State<AppState>,
    archive_path: String,
    entry_paths: Vec<String>,
    bone_names: Vec<String>,
    smooth: f32,
    expressiveness: Option<f32>,
    secondary: Option<f32>,
    sharpness: Option<f32>,
) -> Result<MotionPatchBatchResult, String> {
    let patch_dir = PathBuf::from(state.settings.lock().unwrap().patch_dir.clone());
    let style = batch::MotionStyle {
        smooth,
        expressiveness: expressiveness.unwrap_or(0.0),
        secondary: secondary.unwrap_or(0.0),
        sharpness: sharpness.unwrap_or(0.0),
    };
    batch::export_motion_patch_batch(&PathBuf::from(archive_path), &entry_paths, &bone_names, style, &patch_dir, "compiled")
        .map(|(p, failed)| MotionPatchBatchResult { patch: p.to_string_lossy().into_owned(), failed })
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStatsDto {
    texture_total: usize,
    texture_processed: usize,
    archive_count: Option<usize>,
    game_archive_total_bytes: Option<u64>,
    output_dir_size_bytes: u64,
    models_available: usize,
    app_version: String,
}

/// Mirrors the dev-bridge's `/api/stats` (see `vite-dev-api.ts`) so the packaged app's
/// Dashboard actually has real numbers instead of the hard "not implemented" throw it shipped
/// with (owner-reported: "Якщо запускати exe - дашборд не працює").
#[tauri::command]
fn get_stats(state: State<AppState>) -> AppStatsDto {
    let (game_exe, out_dir) = {
        let s = state.settings.lock().unwrap();
        (s.game_exe.clone(), PathBuf::from(s.output_dir.clone()))
    };

    let texture_total = batch::list_library(&out_dir).map(|v| v.len()).unwrap_or(0);
    let status = logic::load_review_status(&logic::review_status_path(&out_dir));
    let texture_processed = status.len();

    let (archive_count, game_archive_total_bytes) = match &game_exe {
        Some(exe) => match logic::discover_game(&PathBuf::from(exe)) {
            Ok(discovery) => {
                let paths: Vec<PathBuf> = discovery.archives.iter().map(|a| a.path.clone()).collect();
                (Some(discovery.archives.len()), Some(logic::sum_file_sizes(&paths)))
            }
            Err(_) => (None, None),
        },
        None => (None, None),
    };

    let models_available = game_exe
        .as_ref()
        .and_then(|exe| batch::list_meshes(&PathBuf::from(exe)).ok())
        .map(|v| v.len())
        .unwrap_or(0);

    AppStatsDto {
        texture_total,
        texture_processed,
        archive_count,
        game_archive_total_bytes,
        output_dir_size_bytes: logic::dir_size_bytes(&out_dir),
        models_available,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Starts the remote-access HTTP server + `cloudflared` tunnel (see `remote.rs`). Idempotent —
/// calling it while already running just returns the current status instead of starting a
/// second server.
#[tauri::command(rename_all = "camelCase")]
fn start_remote_access(app: AppHandle, state: State<RemoteState>) -> Result<remote::RemoteStatusDto, String> {
    remote::start(app, &state)
}

#[tauri::command(rename_all = "camelCase")]
fn stop_remote_access(state: State<RemoteState>) {
    remote::stop(&state);
}

#[tauri::command(rename_all = "camelCase")]
fn get_remote_status(state: State<RemoteState>) -> remote::RemoteStatusDto {
    remote::status(&state)
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let settings_path = logic::settings_path(&config_dir);
            let default = logic::default_settings_for(&logic::home_dir());
            let settings = logic::load_settings(&settings_path, default);
            app.manage(AppState {
                settings: Mutex::new(settings),
                settings_path,
            });
            app.manage(RemoteState::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            pick_game_path,
            pick_folder,
            check_game,
            list_library,
            read_texture_data_url,
            read_edited_data_url,
            texture_meta,
            list_meshes,
            mesh_to_obj,
            list_actors,
            actor_to_obj,
            list_motions,
            mesh_texture_refs,
            actor_skeleton,
            motion_tracks,
            actor_skinned_mesh,
            regenerate_texture,
            export_texture,
            import_edited_texture,
            review_queue,
            set_review_status,
            build_patches,
            install_patches,
            uninstall_patches,
            export_motion_patch,
            export_motion_patch_batch,
            export_double_rate_motion_patch,
            get_stats,
            start_remote_access,
            stop_remote_access,
            get_remote_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running RisenLab UI");
}
