#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri shell for RisenLab. Every non-trivial piece of logic lives in a plain function
//! (below, in the `logic` module) with its own unit test — `#[tauri::command]` wrappers are
//! kept as thin as possible (open state, call a logic function, map errors to `String`)
//! since they need a running Tauri app to exercise directly.

mod logic;

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{Manager, State};

use logic::AppSettings;
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
) -> Result<Vec<risenlab::xmot::BoneMotion>, String> {
    batch::motion_tracks(&PathBuf::from(archive_path), &entry_path, &bone_names)
        .map(|tracks| match smooth {
            Some(s) if s > 0.0 => risenlab::xmot::smooth_tracks(&tracks, s),
            _ => tracks,
        })
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn actor_skinned_mesh(archive_path: String, entry_path: String) -> Result<risenlab::xmesh_skin::SkinnedMesh, String> {
    batch::actor_skinned_mesh(&PathBuf::from(archive_path), &entry_path).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn regenerate_texture(state: State<AppState>, png_rel: String, scale: Option<u32>) -> Result<(), String> {
    let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
    batch::regenerate(&out_dir, &png_rel, scale.unwrap_or(2), risenlab::batch::RegenEngine::Auto)
        .map(|_| ())
        .map_err(|e| e.to_string())
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
    strength: f32,
) -> Result<String, String> {
    let patch_dir = PathBuf::from(state.settings.lock().unwrap().patch_dir.clone());
    batch::export_motion_patch(&PathBuf::from(archive_path), &entry_path, &bone_names, strength, &patch_dir, "compiled")
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            pick_game_path,
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
            review_queue,
            set_review_status,
            build_patches,
            install_patches,
            uninstall_patches,
            export_motion_patch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running RisenLab UI");
}
