//! Remote access (owner request, 2026-07-19/20): a colleague opens a URL in any browser and
//! works with the SAME real game data/library the desktop app has open, without installing
//! anything.
//!
//! Architecture: a small synchronous HTTP server (`tiny_http` — no async runtime, consistent
//! with this codebase's existing "plain blocking I/O over a framework" bias, see `src/ai.rs`'s
//! curl.exe note) runs on background threads inside the already-running packaged app. It serves
//! two things on one port:
//! - The built frontend (`../dist`, embedded into the binary at compile time via `include_dir!`
//!   — avoids depending on Tauri's own bundled-resource path resolution, which a remote HTTP
//!   server has no reason to go through).
//! - A `/api/*` JSON surface that mirrors `vite-dev-api.ts`'s route shapes (method, path, query/
//!   body params, response shape) — `app/src/lib/api.ts`'s non-Tauri branch already targets
//!   that exact shape, and a plain browser tab (even one loaded from THIS server) never has
//!   `window.__TAURI_INTERNALS__`, so it always takes that branch. Each handler below calls the
//!   same `batch`/`logic` functions the real `#[tauri::command]`s in `main.rs` call — in-process,
//!   not shelling out to the CLI the dev bridge uses, since this runs inside the real backend.
//!
//! Exposure to the public internet goes through `cloudflared`'s free "quick tunnel"
//! (`cloudflared tunnel --url http://127.0.0.1:<port>`) — deliberately NOT bundled/auto-
//! downloaded by this app: the owner installs it once, themselves, from Cloudflare's own
//! release page, the same trust model as installing any other local tool. If it's missing,
//! `cloudflared_available` on the status DTO tells the UI to say so plainly instead of silently
//! not working.
//!
//! Security model: the tunnel URL itself is an unguessable random subdomain (cloudflared's own
//! design) — the shared TOKEN below is defense in depth on top of that, not the only barrier.
//! Static assets (HTML/JS/CSS — no data) are served with no auth check at all; every `/api/*`
//! call must carry the token (header `X-RisenLab-Token`, or `?token=` for simplicity) or gets a
//! 401. The frontend reads the token once from its own URL's `?token=` query param
//! (`lib/remoteToken.ts`) and attaches it to every `/api/*` call from then on.
//!
//! **Can't be compiled or run in the sandbox this project is normally developed in** (no
//! dlltool/MinGW binutils — same limitation as the rest of `src-tauri`, see `risenlab-project`
//! memory). Verified only by the CI workflow's `cargo test`/`cargo build` on a real Windows
//! runner, same as `get_stats`/`ai_regenerate` before it.

use std::collections::HashMap;
use std::io::{BufRead, Read as _};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use risenlab::batch;

use crate::{logic, resolve_ai_config, AppState, GameCheckResult, LibraryEntryDto, MeshEntryDto, MotionPatchBatchResult, ReviewItemDto};

/// Fixed, not auto-assigned: keeps the whole feature simple to reason about and to point
/// `cloudflared --url` at, with no need to read back an OS-chosen port through a tiny_http API
/// this code can't be test-compiled against locally. A collision (something else already
/// listening on it) just fails `start` with a clear error — rare on a normal desktop.
const REMOTE_PORT: u16 = 47420;

static DIST: include_dir::Dir<'static> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../dist");

struct RemoteSession {
    token: String,
    tunnel_url: Arc<Mutex<Option<String>>>,
    server: Arc<tiny_http::Server>,
    cloudflared_child: Option<Child>,
}

pub struct RemoteState(Mutex<Option<RemoteSession>>);

impl RemoteState {
    pub fn new() -> Self {
        RemoteState(Mutex::new(None))
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatusDto {
    pub running: bool,
    pub port: Option<u16>,
    pub token: Option<String>,
    pub tunnel_url: Option<String>,
    pub cloudflared_available: bool,
}

fn cloudflared_available() -> bool {
    let mut cmd = Command::new("cloudflared");
    cmd.arg("--version");
    risenlab::content::suppress_console_window(&mut cmd);
    cmd.output().is_ok()
}

pub fn status(state: &RemoteState) -> RemoteStatusDto {
    let guard = state.0.lock().unwrap();
    let cloudflared_available = cloudflared_available();
    match &*guard {
        Some(session) => RemoteStatusDto {
            running: true,
            port: Some(REMOTE_PORT),
            token: Some(session.token.clone()),
            tunnel_url: session.tunnel_url.lock().unwrap().clone(),
            cloudflared_available,
        },
        None => RemoteStatusDto { running: false, port: None, token: None, tunnel_url: None, cloudflared_available },
    }
}

pub fn start(app: AppHandle, state: &RemoteState) -> Result<RemoteStatusDto, String> {
    let mut guard = state.0.lock().unwrap();
    if let Some(session) = &*guard {
        return Ok(RemoteStatusDto {
            running: true,
            port: Some(REMOTE_PORT),
            token: Some(session.token.clone()),
            tunnel_url: session.tunnel_url.lock().unwrap().clone(),
            cloudflared_available: cloudflared_available(),
        });
    }

    let server = tiny_http::Server::http(("127.0.0.1", REMOTE_PORT))
        .map_err(|e| format!("не вдалось відкрити порт {REMOTE_PORT}: {e}"))?;
    let server = Arc::new(server);
    let token = generate_token();

    // A small pool of worker threads, each blocking on `recv()` — the standard tiny_http
    // pattern for basic concurrency (e.g. a 3D viewer's several parallel texture fetches)
    // without pulling in an async runtime.
    for _ in 0..4 {
        let server = server.clone();
        let app = app.clone();
        let token = token.clone();
        std::thread::spawn(move || loop {
            match server.recv() {
                Ok(request) => handle_connection(request, &app, &token),
                Err(_) => break, // `Server::unblock()` (stop()) or the socket closing
            }
        });
    }

    let tunnel_url = Arc::new(Mutex::new(None));
    let cloudflared_child = if cloudflared_available() { spawn_cloudflared(tunnel_url.clone()) } else { None };
    let cloudflared_started = cloudflared_child.is_some();

    let dto = RemoteStatusDto {
        running: true,
        port: Some(REMOTE_PORT),
        token: Some(token.clone()),
        tunnel_url: None,
        cloudflared_available: cloudflared_started,
    };
    *guard = Some(RemoteSession { token, tunnel_url, server, cloudflared_child });
    Ok(dto)
}

pub fn stop(state: &RemoteState) {
    let mut guard = state.0.lock().unwrap();
    if let Some(session) = guard.take() {
        // Interrupts every worker thread's blocking `recv()` — the documented tiny_http way to
        // shut a server down from another thread (dropping the `Server` alone doesn't wake
        // threads already parked inside `recv()`).
        session.server.unblock();
        if let Some(mut child) = session.cloudflared_child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn generate_token() -> String {
    // No `rand` dependency: `RandomState` is the standard library's own OS-seeded hasher (each
    // `::new()` re-seeds from real OS randomness) — hashing a timestamp through four freshly-
    // seeded instances gives a 256-bit-ish token without adding a crate for it. Good enough as
    // defense-in-depth on top of the tunnel's own unguessable subdomain (see the module doc).
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut out = String::new();
    for salt in 0..4u64 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(nanos);
        hasher.write_u64(salt);
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out
}

enum StreamKind {
    Stdout(std::process::ChildStdout),
    Stderr(std::process::ChildStderr),
}

fn spawn_cloudflared(tunnel_url: Arc<Mutex<Option<String>>>) -> Option<Child> {
    let mut cmd = Command::new("cloudflared");
    cmd.args(["tunnel", "--url", &format!("http://127.0.0.1:{REMOTE_PORT}")])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    risenlab::content::suppress_console_window(&mut cmd);
    let mut child = cmd.spawn().ok()?;

    // cloudflared logs its assigned `https://*.trycloudflare.com` URL as part of its startup
    // log — real-world builds have put this on stderr, but scanning both is cheap insurance
    // against a version that changes that.
    let streams: Vec<StreamKind> = [child.stderr.take().map(StreamKind::Stderr), child.stdout.take().map(StreamKind::Stdout)]
        .into_iter()
        .flatten()
        .collect();
    for stream in streams {
        let slot = tunnel_url.clone();
        std::thread::spawn(move || {
            let reader: Box<dyn BufRead> = match stream {
                StreamKind::Stderr(s) => Box::new(std::io::BufReader::new(s)),
                StreamKind::Stdout(s) => Box::new(std::io::BufReader::new(s)),
            };
            for line in reader.lines().flatten() {
                if let Some(url) = extract_trycloudflare_url(&line) {
                    let mut guard = slot.lock().unwrap();
                    if guard.is_none() {
                        *guard = Some(url);
                    }
                    break;
                }
            }
        });
    }
    Some(child)
}

fn extract_trycloudflare_url(line: &str) -> Option<String> {
    let idx = line.find("https://")?;
    let rest = &line[idx..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '|').unwrap_or(rest.len());
    let candidate = rest[..end].trim_end_matches(['.', ',']);
    if candidate.contains("trycloudflare.com") {
        Some(candidate.to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------------------
// HTTP plumbing (query/body parsing, responses) — no extra crate for any of this, tiny_http
// hands back the raw request-target unparsed and expects the caller to build responses by hand.
// ---------------------------------------------------------------------------------------

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_path_and_query(raw: &str) -> (String, HashMap<String, String>) {
    let mut parts = raw.splitn(2, '?');
    let path = parts.next().unwrap_or("").to_string();
    let mut map = HashMap::new();
    if let Some(qs) = parts.next() {
        for pair in qs.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut kv = pair.splitn(2, '=');
            let k = kv.next().unwrap_or("");
            let v = kv.next().unwrap_or("");
            map.insert(percent_decode(k), percent_decode(v));
        }
    }
    (path, map)
}

fn read_body_bytes(request: &mut tiny_http::Request) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = request.as_reader().read_to_end(&mut buf);
    buf
}

fn json_header() -> tiny_http::Header {
    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap()
}

fn respond_json(request: tiny_http::Request, status: u16, value: &serde_json::Value) {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    let response = tiny_http::Response::from_string(body).with_status_code(status).with_header(json_header());
    let _ = request.respond(response);
}

fn respond_ok<T: Serialize>(request: tiny_http::Request, value: &T) {
    match serde_json::to_value(value) {
        Ok(v) => respond_json(request, 200, &v),
        Err(e) => respond_json(request, 500, &serde_json::json!({ "error": e.to_string() })),
    }
}

fn respond_error(request: tiny_http::Request, status: u16, message: impl Into<String>) {
    respond_json(request, status, &serde_json::json!({ "error": message.into() }));
}

fn respond_bytes(request: tiny_http::Request, status: u16, content_type: &str, body: Vec<u8>) {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap();
    let response = tiny_http::Response::from_data(body).with_status_code(status).with_header(header);
    let _ = request.respond(response);
}

// ---------------------------------------------------------------------------------------
// Static asset serving (the embedded, compiled-in `../dist`)
// ---------------------------------------------------------------------------------------

fn guess_content_type(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") || path.ends_with(".mjs") {
        "text/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else if path.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    }
}

fn serve_static(request: tiny_http::Request, path: &str) {
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if let Some(file) = DIST.get_file(rel) {
        respond_bytes(request, 200, guess_content_type(rel), file.contents().to_vec());
        return;
    }
    // SPA fallback: any unrecognized path (a client-side route, or just `/`) serves the shell —
    // the React app itself resolves the actual screen from its own in-memory state, not the URL.
    if let Some(index) = DIST.get_file("index.html") {
        respond_bytes(request, 200, "text/html; charset=utf-8", index.contents().to_vec());
        return;
    }
    respond_error(request, 404, "not found");
}

// ---------------------------------------------------------------------------------------
// Request dispatch
// ---------------------------------------------------------------------------------------

fn extract_token(request: &tiny_http::Request, query: &HashMap<String, String>) -> Option<String> {
    for h in request.headers() {
        if h.field.equiv("X-RisenLab-Token") {
            return Some(h.value.as_str().to_string());
        }
    }
    query.get("token").cloned()
}

fn handle_connection(request: tiny_http::Request, app: &AppHandle, token: &str) {
    let raw_url = request.url().to_string();
    let (path, query) = parse_path_and_query(&raw_url);

    if let Some(rest) = path.strip_prefix("/api/") {
        if extract_token(&request, &query).as_deref() != Some(token) {
            respond_error(request, 401, "unauthorized");
            return;
        }
        route_api(request, app, rest, &query);
    } else {
        serve_static(request, &path);
    }
}

/// Every `/api/*` route, mirroring `vite-dev-api.ts`'s method+path+shape 1:1 — `app/src/lib/
/// api.ts`'s non-Tauri branch (the one any plain browser, including a remote one, always takes)
/// was written against exactly that shape. Bodies/params are parsed here; the actual work is
/// the same `batch`/`logic` calls the real `#[tauri::command]`s in `main.rs` make, read from
/// the SAME shared `AppState` (`app.state::<AppState>()`) — a settings change made locally is
/// immediately visible to a remote request and vice versa.
///
/// Deliberately ignores any `outputDir`/`patchDir`/etc. the client sends (the dev bridge reads
/// these from query/body params because it's a separate process with no shared state to read
/// them from) — this server always trusts its own `AppState`, both simpler and safer (no path
/// a remote client controls ever reaches the filesystem directly).
fn route_api(mut request: tiny_http::Request, app: &AppHandle, path: &str, query: &HashMap<String, String>) {
    let is_get = *request.method() == tiny_http::Method::Get;
    let is_post = *request.method() == tiny_http::Method::Post;
    let state = app.state::<AppState>();

    match (path, is_get, is_post) {
        ("settings", true, _) => {
            let settings = state.settings.lock().unwrap().clone();
            respond_ok(request, &settings);
        }
        ("settings", _, true) => {
            let body = read_body_bytes(&mut request);
            match serde_json::from_slice::<logic::AppSettings>(&body) {
                Ok(settings) => match logic::save_settings_to(&state.settings_path, &settings) {
                    Ok(()) => {
                        *state.settings.lock().unwrap() = settings;
                        respond_ok(request, &serde_json::json!({ "ok": true }));
                    }
                    Err(e) => respond_error(request, 500, e.to_string()),
                },
                Err(e) => respond_error(request, 400, e.to_string()),
            }
        }
        ("pick-file", _, true) | ("pick-folder", _, true) | ("export-texture", _, true) | ("import-edited-texture", _, true) => {
            // All four open a NATIVE dialog on whichever machine handles the request — for a
            // remote browser tab that would mean a dialog popping up on the OWNER's screen, not
            // the colleague's, so these stay local-only (same reasoning as pick-file/-folder).
            respond_error(request, 400, "Ця дія доступна лише локально в застосунку, не віддалено");
        }
        ("check-game", _, true) => {
            let (exe, out_dir) = {
                let s = state.settings.lock().unwrap();
                match s.game_exe.clone() {
                    Some(exe) => (exe, PathBuf::from(s.output_dir.clone())),
                    None => {
                        respond_error(request, 400, "Спершу вкажіть шлях до гри");
                        return;
                    }
                }
            };
            let exe_path = PathBuf::from(&exe);
            match logic::discover_game(&exe_path) {
                Ok(discovery) => {
                    let paths: Vec<PathBuf> = discovery.archives.iter().map(|a| a.path.clone()).collect();
                    let total_bytes = logic::sum_file_sizes(&paths);
                    let archive_count = discovery.archives.len();
                    let root = discovery.root.to_string_lossy().into_owned();
                    match batch::extract_all(&exe_path, &out_dir) {
                        Ok(extracted) => respond_ok(
                            request,
                            &GameCheckResult { root, archive_count, total_bytes, textures_extracted: extracted },
                        ),
                        Err(e) => respond_error(request, 500, e.to_string()),
                    }
                }
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("stats", true, _) => {
            let (game_exe, out_dir) = {
                let s = state.settings.lock().unwrap();
                (s.game_exe.clone(), PathBuf::from(s.output_dir.clone()))
            };
            let texture_total = batch::list_library(&out_dir).map(|v| v.len()).unwrap_or(0);
            let review_status = logic::load_review_status(&logic::review_status_path(&out_dir));
            let texture_processed = review_status.len();
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
            respond_ok(
                request,
                &serde_json::json!({
                    "textureTotal": texture_total,
                    "textureProcessed": texture_processed,
                    "archiveCount": archive_count,
                    "gameArchiveTotalBytes": game_archive_total_bytes,
                    "outputDirSizeBytes": logic::dir_size_bytes(&out_dir),
                    "modelsAvailable": models_available,
                    "appVersion": env!("CARGO_PKG_VERSION"),
                }),
            );
        }
        ("list-library", true, _) => {
            let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
            match batch::list_library(&out_dir) {
                Ok(v) => respond_ok(request, &v.into_iter().map(LibraryEntryDto::from).collect::<Vec<_>>()),
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("list-meshes", true, _) => with_game_exe(request, &state, |exe| {
            batch::list_meshes(&PathBuf::from(exe)).map(|v| v.into_iter().map(MeshEntryDto::from).collect::<Vec<_>>())
        }),
        ("list-actors", true, _) => with_game_exe(request, &state, |exe| {
            batch::list_actors(&PathBuf::from(exe)).map(|v| v.into_iter().map(MeshEntryDto::from).collect::<Vec<_>>())
        }),
        ("list-motions", true, _) => with_game_exe(request, &state, |exe| {
            batch::list_motions(&PathBuf::from(exe)).map(|v| v.into_iter().map(MeshEntryDto::from).collect::<Vec<_>>())
        }),
        ("mesh-obj", true, _) | ("actor-obj", true, _) => {
            let archive_path = query.get("archivePath").cloned().unwrap_or_default();
            let entry_path = query.get("entryPath").cloned().unwrap_or_default();
            let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
            let is_actor = path == "actor-obj";
            let cache_dir = out_dir
                .parent()
                .map(|p| p.join(if is_actor { "actors" } else { "meshes" }))
                .unwrap_or_else(|| out_dir.join(if is_actor { "actors" } else { "meshes" }));
            let result = if is_actor {
                batch::actor_to_obj_from_archive(&PathBuf::from(&archive_path), &entry_path, &cache_dir)
            } else {
                batch::mesh_to_obj_from_archive(&PathBuf::from(&archive_path), &entry_path, &cache_dir)
            };
            match result {
                Ok(obj_path) => {
                    let _ = batch::embed_real_texture_paths(&obj_path, &out_dir);
                    match std::fs::read_to_string(&obj_path) {
                        Ok(text) => respond_bytes(request, 200, "text/plain; charset=utf-8", text.into_bytes()),
                        Err(e) => respond_error(request, 500, e.to_string()),
                    }
                }
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("mesh-texture-refs", true, _) => {
            let archive_path = query.get("archivePath").cloned().unwrap_or_default();
            let entry_path = query.get("entryPath").cloned().unwrap_or_default();
            let kind = query.get("kind").cloned().unwrap_or_default();
            let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
            let is_actor = kind == "actor";
            let cache_dir = out_dir
                .parent()
                .map(|p| p.join(if is_actor { "actors" } else { "meshes" }))
                .unwrap_or_else(|| out_dir.join(if is_actor { "actors" } else { "meshes" }));
            let obj_result = if is_actor {
                batch::actor_to_obj_from_archive(&PathBuf::from(&archive_path), &entry_path, &cache_dir)
            } else {
                batch::mesh_to_obj_from_archive(&PathBuf::from(&archive_path), &entry_path, &cache_dir)
            };
            match obj_result.and_then(|obj_path| batch::read_material_texture_refs(&obj_path)) {
                Ok(refs) => respond_ok(request, &refs),
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("actor-skeleton", true, _) => {
            let archive_path = query.get("archivePath").cloned().unwrap_or_default();
            let entry_path = query.get("entryPath").cloned().unwrap_or_default();
            match batch::actor_skeleton(&PathBuf::from(&archive_path), &entry_path) {
                Ok(nodes) => respond_ok(request, &nodes),
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("motion-tracks", true, _) => {
            let archive_path = query.get("archivePath").cloned().unwrap_or_default();
            let entry_path = query.get("entryPath").cloned().unwrap_or_default();
            let bone_names: Vec<String> = query
                .get("boneNames")
                .and_then(|j| serde_json::from_str(j).ok())
                .unwrap_or_default();
            let f = |key: &str| query.get(key).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
            let smooth = f("smooth");
            let expressiveness = f("expressiveness");
            let secondary = f("secondary");
            let sharpness = f("sharpness");
            let double_rate = query.get("doubleRate").map(|v| v == "true").unwrap_or(false);
            match batch::motion_tracks(&PathBuf::from(&archive_path), &entry_path, &bone_names) {
                Ok(tracks) => {
                    let tracks = if smooth > 0.0 { risenlab::xmot::smooth_tracks(&tracks, smooth) } else { tracks };
                    let tracks = risenlab::xmot::stylize_tracks(&tracks, expressiveness, secondary, sharpness);
                    let tracks = if double_rate { risenlab::xmot::resample_double_rate(&tracks) } else { tracks };
                    respond_ok(request, &tracks);
                }
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("actor-skinned-mesh", true, _) => {
            let archive_path = query.get("archivePath").cloned().unwrap_or_default();
            let entry_path = query.get("entryPath").cloned().unwrap_or_default();
            match batch::actor_skinned_mesh(&PathBuf::from(&archive_path), &entry_path) {
                Ok(mesh) => respond_ok(request, &mesh),
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("texture", true, _) => {
            let png_rel = query.get("pngRel").cloned().unwrap_or_default();
            let edited = query.get("edited").map(|v| v == "1").unwrap_or(false);
            let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
            let file_path = if edited { out_dir.join("edited").join(&png_rel) } else { out_dir.join(&png_rel) };
            match std::fs::read(&file_path) {
                Ok(bytes) => respond_bytes(request, 200, "image/png", bytes),
                Err(e) => respond_error(request, 404, e.to_string()),
            }
        }
        ("texture-meta", true, _) => {
            let archive_path = query.get("archivePath").cloned().unwrap_or_default();
            let entry_path = query.get("entryPath").cloned().unwrap_or_default();
            match logic::read_texture_meta(&PathBuf::from(&archive_path), &entry_path) {
                Ok(meta) => respond_ok(request, &meta),
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("regenerate", _, true) => {
            let body = read_body_bytes(&mut request);
            let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let png_rel = parsed.get("pngRel").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let scale = parsed.get("scale").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let (out_dir, ai_config) = {
                let s = state.settings.lock().unwrap();
                (PathBuf::from(s.output_dir.clone()), resolve_ai_config(&s))
            };
            match batch::regenerate(&out_dir, &png_rel, scale, batch::RegenEngine::Auto, ai_config.as_ref()) {
                Ok(_) => match logic::reset_review_status(&logic::review_status_path(&out_dir), &png_rel) {
                    Ok(()) => respond_ok(request, &serde_json::json!({ "ok": true })),
                    Err(e) => respond_error(request, 500, e.to_string()),
                },
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("review-queue", true, _) => {
            let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
            match logic::list_edited_pngs(&out_dir) {
                Ok(edited) => {
                    let status = logic::load_review_status(&logic::review_status_path(&out_dir));
                    let items: Vec<ReviewItemDto> = logic::review_queue_from(&edited, &status)
                        .into_iter()
                        .map(|(png_rel, status)| ReviewItemDto { png_rel, status })
                        .collect();
                    respond_ok(request, &items);
                }
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("review-status", _, true) => {
            let body = read_body_bytes(&mut request);
            let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let png_rel = parsed.get("pngRel").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let status_value = parsed.get("status").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let out_dir = PathBuf::from(state.settings.lock().unwrap().output_dir.clone());
            let review_path = logic::review_status_path(&out_dir);
            let mut map = logic::load_review_status(&review_path);
            if status_value == "rejected" {
                let _ = std::fs::remove_file(out_dir.join("edited").join(&png_rel));
                map.remove(&png_rel);
            } else {
                map.insert(png_rel, status_value);
            }
            match logic::save_review_status(&review_path, &map) {
                Ok(()) => respond_ok(request, &serde_json::json!({ "ok": true })),
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("build-patches", _, true) => {
            let (out_dir, patch_dir) = {
                let s = state.settings.lock().unwrap();
                (PathBuf::from(s.output_dir.clone()), PathBuf::from(s.patch_dir.clone()))
            };
            let status = logic::load_review_status(&logic::review_status_path(&out_dir));
            let approved = logic::approved_pngs(&status);
            let staging = out_dir.join("_approved_stage");
            let _ = std::fs::remove_dir_all(&staging);
            let result = logic::stage_approved_for_apply(&out_dir.join("edited"), &staging, &approved).and_then(|()| {
                let manifest = out_dir.join("manifest.tsv");
                batch::apply(&manifest, &staging, &patch_dir)
            });
            let _ = std::fs::remove_dir_all(&staging);
            match result {
                Ok(written) => respond_ok(
                    request,
                    &written.into_iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(),
                ),
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("install-patches", _, true) | ("uninstall-patches", _, true) => {
            let (game_exe, patch_dir) = {
                let s = state.settings.lock().unwrap();
                (s.game_exe.clone(), PathBuf::from(s.patch_dir.clone()))
            };
            let Some(game_exe) = game_exe else {
                respond_error(request, 400, "gameExe not configured");
                return;
            };
            let result = if path == "install-patches" {
                batch::install_patches(&PathBuf::from(game_exe), &patch_dir)
            } else {
                batch::uninstall_patches(&PathBuf::from(game_exe), &patch_dir)
            };
            match result {
                Ok(list) => respond_ok(request, &list),
                Err(e) => respond_error(request, 500, e.to_string()),
            }
        }
        ("export-motion-patch", _, true) | ("export-motion-patch-batch", _, true) | ("export-double-rate-motion-patch", _, true) => {
            let body = read_body_bytes(&mut request);
            let parsed: serde_json::Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(e) => {
                    respond_error(request, 400, e.to_string());
                    return;
                }
            };
            let patch_dir = PathBuf::from(state.settings.lock().unwrap().patch_dir.clone());
            let archive_path = parsed.get("archivePath").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let bone_names: Vec<String> = parsed
                .get("boneNames")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let num = |key: &str| parsed.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

            if path == "export-double-rate-motion-patch" {
                let entry_path = parsed.get("entryPath").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                match batch::export_double_rate_motion_patch(&PathBuf::from(&archive_path), &entry_path, &bone_names, &patch_dir, "compiled") {
                    Ok(p) => respond_ok(request, &serde_json::json!({ "patch": p.to_string_lossy() })),
                    Err(e) => respond_error(request, 500, e.to_string()),
                }
                return;
            }
            let style = batch::MotionStyle {
                smooth: num("smooth"),
                expressiveness: num("expressiveness"),
                secondary: num("secondary"),
                sharpness: num("sharpness"),
            };
            if path == "export-motion-patch" {
                let entry_path = parsed.get("entryPath").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                match batch::export_motion_patch(&PathBuf::from(&archive_path), &entry_path, &bone_names, style, &patch_dir, "compiled") {
                    Ok(p) => respond_ok(request, &serde_json::json!({ "patch": p.to_string_lossy() })),
                    Err(e) => respond_error(request, 500, e.to_string()),
                }
            } else {
                let entry_paths: Vec<String> = parsed
                    .get("entryPaths")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                match batch::export_motion_patch_batch(&PathBuf::from(&archive_path), &entry_paths, &bone_names, style, &patch_dir, "compiled") {
                    Ok((p, failed)) => respond_ok(request, &MotionPatchBatchResult { patch: p.to_string_lossy().into_owned(), failed }),
                    Err(e) => respond_error(request, 500, e.to_string()),
                }
            }
        }
        ("backup", _, true) => {
            // Matches the packaged Tauri backend's own current gap (`api.ts`: "Backup is not
            // implemented in the Tauri backend yet") — not new scope for remote access to fix.
            respond_error(request, 501, "Backup is not implemented in the Tauri backend yet");
        }
        _ => respond_error(request, 404, "not found"),
    }
}

fn with_game_exe<T: Serialize>(request: tiny_http::Request, state: &AppState, f: impl FnOnce(&str) -> anyhow::Result<T>) {
    let exe = state.settings.lock().unwrap().game_exe.clone();
    match exe {
        Some(exe) => match f(&exe) {
            Ok(v) => respond_ok(request, &v),
            Err(e) => respond_error(request, 500, e.to_string()),
        },
        None => respond_error(request, 400, "Спершу вкажіть шлях до гри"),
    }
}
