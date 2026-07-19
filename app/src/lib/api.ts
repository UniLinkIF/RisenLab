// Thin wrapper over the backend commands. Two real backends, no fabricated/mock data in
// either:
//  - Inside the real Tauri app (`isTauri()` true): calls the Rust commands in
//    app/src-tauri/src/main.rs directly.
//  - In a plain browser dev preview (`npm run dev`, e.g. while the Tauri shell can't be
//    compiled in this environment): calls the local dev-only HTTP API (vite-dev-api.ts),
//    which shells out to the real `risenlab.exe` CLI and does real filesystem work — same
//    game, same files, same picker dialogs, nothing invented.
import type { ActorEntry, AppSettings, AppStats, BoneMotion, GameCheckResult, LibraryEntry, MaterialTextureRefs, MeshEntry, MotionEntry, ReviewItem, ReviewStatus, SkeletonNode, SkinnedMeshData, TextureMeta } from "./types";
import { memoizeAsync } from "./cache";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const mod = await import("@tauri-apps/api/core");
  return mod.invoke<T>(cmd, args);
}

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api/${path}`, init);
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error ?? `${path} failed (${res.status})`);
  }
  return res.json() as Promise<T>;
}

function postJson(path: string, body: unknown): Promise<Response> {
  return fetch(`/api/${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  }).then((res) => {
    if (!res.ok) throw new Error(`${path} failed (${res.status})`);
    return res;
  });
}

export async function getSettings(): Promise<AppSettings> {
  if (isTauri()) return invoke<AppSettings>("get_settings");
  return api<AppSettings>("settings");
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  if (isTauri()) return invoke("save_settings", { settings });
  await postJson("settings", settings);
}

export async function pickGamePath(): Promise<string | null> {
  if (isTauri()) return invoke<string | null>("pick_game_path");
  const { path } = await api<{ path: string | null }>("pick-file", { method: "POST" });
  return path;
}

export async function pickFolder(): Promise<string | null> {
  const { path } = await api<{ path: string | null }>("pick-folder", { method: "POST" });
  return path;
}

export async function checkGame(): Promise<GameCheckResult> {
  if (isTauri()) return invoke<GameCheckResult>("check_game");
  return api<GameCheckResult>("check-game", { method: "POST" });
}

export async function listLibrary(): Promise<LibraryEntry[]> {
  if (isTauri()) return invoke<LibraryEntry[]>("list_library");
  const settings = await getSettings();
  return api<LibraryEntry[]>(`list-library?outputDir=${encodeURIComponent(settings.outputDir)}`);
}

export async function listMeshes(): Promise<MeshEntry[]> {
  if (isTauri()) return invoke<MeshEntry[]>("list_meshes");
  return api<MeshEntry[]>("list-meshes");
}

// Converting a mesh to .obj shells out to mimicry-helper.exe, so it's worth caching the URL
// per entry the same way texture reads are — reopening an already-viewed model is instant.
const meshObjUrlCache = new Map<string, Promise<string>>();

export async function meshObjUrl(archivePath: string, entryPath: string): Promise<string> {
  const key = `${archivePath}::${entryPath}`;
  return memoizeAsync(meshObjUrlCache, key, async () => {
    if (isTauri()) {
      const text = await invoke<string>("mesh_to_obj", { archivePath, entryPath });
      return URL.createObjectURL(new Blob([text], { type: "text/plain" }));
    }
    return `/api/mesh-obj?archivePath=${encodeURIComponent(archivePath)}&entryPath=${encodeURIComponent(entryPath)}`;
  });
}

export async function listActors(): Promise<ActorEntry[]> {
  if (isTauri()) return invoke<ActorEntry[]>("list_actors");
  return api<ActorEntry[]>("list-actors");
}

// Same lazy-conversion-plus-cache approach as meshObjUrl (see there for why).
const actorObjUrlCache = new Map<string, Promise<string>>();

export async function actorObjUrl(archivePath: string, entryPath: string): Promise<string> {
  const key = `${archivePath}::${entryPath}`;
  return memoizeAsync(actorObjUrlCache, key, async () => {
    if (isTauri()) {
      const text = await invoke<string>("actor_to_obj", { archivePath, entryPath });
      return URL.createObjectURL(new Blob([text], { type: "text/plain" }));
    }
    return `/api/actor-obj?archivePath=${encodeURIComponent(archivePath)}&entryPath=${encodeURIComponent(entryPath)}`;
  });
}

export async function listMotions(): Promise<MotionEntry[]> {
  if (isTauri()) return invoke<MotionEntry[]>("list_motions");
  return api<MotionEntry[]>("list-motions");
}

// Real diffuse/normal texture file names a mesh/actor's own material references — see
// `MaterialTextureRefs`. Cached like the .obj URLs above (same conversion cost to get there).
const textureRefsCache = new Map<string, Promise<MaterialTextureRefs>>();

function meshOrActorTextureRefs(archivePath: string, entryPath: string, kind: "mesh" | "actor"): Promise<MaterialTextureRefs> {
  const key = `${kind}::${archivePath}::${entryPath}`;
  return memoizeAsync(textureRefsCache, key, async () => {
    if (isTauri()) return invoke<MaterialTextureRefs>("mesh_texture_refs", { archivePath, entryPath, kind });
    return api<MaterialTextureRefs>(
      `mesh-texture-refs?archivePath=${encodeURIComponent(archivePath)}&entryPath=${encodeURIComponent(entryPath)}&kind=${kind}`,
    );
  });
}

export const meshTextureRefs = (archivePath: string, entryPath: string) => meshOrActorTextureRefs(archivePath, entryPath, "mesh");
export const actorTextureRefs = (archivePath: string, entryPath: string) => meshOrActorTextureRefs(archivePath, entryPath, "actor");

// The skeleton is a small, cheap parse (no mimicry-helper round trip) — still worth caching
// since it's re-fetched every time the motion-track effect below re-runs for the same actor.
const skeletonCache = new Map<string, Promise<SkeletonNode[]>>();

export async function actorSkeleton(archivePath: string, entryPath: string): Promise<SkeletonNode[]> {
  const key = `${archivePath}::${entryPath}`;
  return memoizeAsync(skeletonCache, key, async () => {
    if (isTauri()) return invoke<SkeletonNode[]>("actor_skeleton", { archivePath, entryPath });
    return api<SkeletonNode[]>(
      `actor-skeleton?archivePath=${encodeURIComponent(archivePath)}&entryPath=${encodeURIComponent(entryPath)}`,
    );
  });
}

/** The four independently-toggleable local motion transforms — jitter cleanup plus the three
 * "animation quality" ones (`expressiveness`/`secondary`/`sharpness`, see `xmot::stylize_tracks`
 * on the Rust side). All default to 0 (no-op). `doubleRate` is a separate, PREVIEW-ONLY fifth
 * toggle (`xmot::resample_double_rate`, "🎬 60fps") — it changes key counts, which can't be
 * written back to a real `.xmot` file in place, so `motionTracks` accepts it but the export
 * functions below deliberately don't. */
export interface MotionStyle {
  smooth?: number;
  expressiveness?: number;
  secondary?: number;
  sharpness?: number;
  doubleRate?: boolean;
}

export async function motionTracks(
  archivePath: string,
  entryPath: string,
  boneNames: string[],
  style: MotionStyle = {},
): Promise<BoneMotion[]> {
  const { smooth = 0, expressiveness = 0, secondary = 0, sharpness = 0, doubleRate = false } = style;
  if (isTauri()) return invoke<BoneMotion[]>("motion_tracks", { archivePath, entryPath, boneNames, smooth, expressiveness, secondary, sharpness, doubleRate });
  const boneNamesJson = encodeURIComponent(JSON.stringify(boneNames));
  return api<BoneMotion[]>(
    `motion-tracks?archivePath=${encodeURIComponent(archivePath)}&entryPath=${encodeURIComponent(entryPath)}&boneNames=${boneNamesJson}&smooth=${smooth}&expressiveness=${expressiveness}&secondary=${secondary}&sharpness=${sharpness}&doubleRate=${doubleRate}`,
  );
}

// A real skinned mesh can be a few hundred KB of JSON (thousands of vertices) — worth caching
// per actor the same way the skeleton is, since re-selecting the same actor is common.
const skinnedMeshCache = new Map<string, Promise<SkinnedMeshData>>();

export async function actorSkinnedMesh(archivePath: string, entryPath: string): Promise<SkinnedMeshData> {
  const key = `${archivePath}::${entryPath}`;
  return memoizeAsync(skinnedMeshCache, key, async () => {
    if (isTauri()) return invoke<SkinnedMeshData>("actor_skinned_mesh", { archivePath, entryPath });
    return api<SkinnedMeshData>(
      `actor-skinned-mesh?archivePath=${encodeURIComponent(archivePath)}&entryPath=${encodeURIComponent(entryPath)}`,
    );
  });
}

// Real image URLs are cached (module-level Map) so revisiting a texture (switching folders,
// returning from AiCompare, reopening the search overlay) is instant instead of re-reading
// the file / re-invoking the backend every time.
const textureUrlCache = new Map<string, Promise<string>>();
const editedUrlCache = new Map<string, Promise<string>>();

export async function readTextureDataUrl(pngRel: string): Promise<string> {
  return memoizeAsync(textureUrlCache, pngRel, async () => {
    if (isTauri()) return invoke<string>("read_texture_data_url", { pngRel });
    const settings = await getSettings();
    return `/api/texture?outputDir=${encodeURIComponent(settings.outputDir)}&pngRel=${encodeURIComponent(pngRel)}`;
  });
}

export async function readEditedDataUrl(pngRel: string): Promise<string> {
  return memoizeAsync(editedUrlCache, pngRel, async () => {
    if (isTauri()) return invoke<string>("read_edited_data_url", { pngRel });
    const settings = await getSettings();
    return `/api/texture?outputDir=${encodeURIComponent(settings.outputDir)}&pngRel=${encodeURIComponent(pngRel)}&edited=1`;
  });
}

export async function textureMeta(archivePath: string, entryPath: string): Promise<TextureMeta> {
  if (isTauri()) return invoke<TextureMeta>("texture_meta", { archivePath, entryPath });
  return api<TextureMeta>(
    `texture-meta?archivePath=${encodeURIComponent(archivePath)}&entryPath=${encodeURIComponent(entryPath)}`,
  );
}

/** scale 0 = smart auto (Rust side): ≤256px textures get 4x, larger get 2x. */
export async function regenerateTexture(pngRel: string, scale = 0): Promise<void> {
  editedUrlCache.delete(pngRel); // the variant just changed — force a fresh read next time
  if (isTauri()) return invoke("regenerate_texture", { pngRel, scale });
  const settings = await getSettings();
  await postJson("regenerate", { outputDir: settings.outputDir, pngRel, scale });
}

export async function reviewQueue(): Promise<ReviewItem[]> {
  if (isTauri()) return invoke<ReviewItem[]>("review_queue");
  const settings = await getSettings();
  return api<ReviewItem[]>(`review-queue?outputDir=${encodeURIComponent(settings.outputDir)}`);
}

export async function setReviewStatus(pngRel: string, status: ReviewStatus): Promise<void> {
  if (status === "rejected") editedUrlCache.delete(pngRel);
  if (isTauri()) return invoke("set_review_status", { pngRel, status });
  const settings = await getSettings();
  await postJson("review-status", { outputDir: settings.outputDir, pngRel, status });
}

export async function buildPatches(): Promise<string[]> {
  if (isTauri()) return invoke<string[]>("build_patches");
  return api<string[]>("build-patches", { method: "POST" });
}

/** Copies every built `.pNN` patch volume into the game's own data directories — the
 * "install my mod" step. Returns the installed `group/name` list. */
export async function installPatches(): Promise<string[]> {
  if (isTauri()) return invoke<string[]>("install_patches");
  return api<string[]>("install-patches", { method: "POST" });
}

/** Removes previously installed patch volumes from the game (only files that also exist in
 * the patch output dir — nothing else is touched). Returns the removed list. */
export async function uninstallPatches(): Promise<string[]> {
  if (isTauri()) return invoke<string[]>("uninstall_patches");
  return api<string[]>("uninstall-patches", { method: "POST" });
}

/** Styles MANY clips the same way (e.g. every animation of one creature) into a SINGLE `.pNN`
 * patch volume. Returns the patch path plus per-clip failures (skipped, not fatal). */
export async function exportMotionPatchBatch(
  archivePath: string,
  entryPaths: string[],
  boneNames: string[],
  style: MotionStyle,
): Promise<{ patch: string; failed: string[] }> {
  const { smooth = 0, expressiveness = 0, secondary = 0, sharpness = 0 } = style;
  if (isTauri()) return invoke<{ patch: string; failed: string[] }>("export_motion_patch_batch", { archivePath, entryPaths, boneNames, smooth, expressiveness, secondary, sharpness });
  return api<{ patch: string; failed: string[] }>("export-motion-patch-batch", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ archivePath, entryPaths, boneNames, smooth, expressiveness, secondary, sharpness }),
  });
}

/** Styles one clip and packs it straight into a fresh animations `.pNN` patch volume —
 * returns the patch file path (install with `installPatches`). */
export async function exportMotionPatch(
  archivePath: string,
  entryPath: string,
  boneNames: string[],
  style: MotionStyle,
): Promise<string> {
  const { smooth = 0, expressiveness = 0, secondary = 0, sharpness = 0 } = style;
  if (isTauri()) return invoke<string>("export_motion_patch", { archivePath, entryPath, boneNames, smooth, expressiveness, secondary, sharpness });
  const res = await api<{ patch: string }>("export-motion-patch", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ archivePath, entryPath, boneNames, smooth, expressiveness, secondary, sharpness }),
  });
  return res.patch;
}

/** The real, exportable "🎬 60fps" — genuinely doubles key rate and resizes the `.xmot` on
 * disk (`xmot::rebuild_motion_file`), unlike `motion-tracks`' `doubleRate` flag, which only
 * ever affects the in-app preview. UNVERIFIED IN-GAME — see the Rust doc comment on
 * `batch::export_double_rate_motion_patch`. Returns the patch file path. */
export async function exportDoubleRateMotionPatch(archivePath: string, entryPath: string, boneNames: string[]): Promise<string> {
  if (isTauri()) return invoke<string>("export_double_rate_motion_patch", { archivePath, entryPath, boneNames });
  const res = await api<{ patch: string }>("export-double-rate-motion-patch", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ archivePath, entryPath, boneNames }),
  });
  return res.patch;
}

export async function backupProject(): Promise<string> {
  if (isTauri()) throw new Error("Backup is not implemented in the Tauri backend yet");
  const { path } = await api<{ path: string }>("backup", { method: "POST" });
  return path;
}

export async function getStats(): Promise<AppStats> {
  if (isTauri()) return invoke<AppStats>("get_stats");
  return api<AppStats>("stats");
}
