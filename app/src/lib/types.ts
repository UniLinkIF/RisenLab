export interface LibraryEntry {
  group: string;
  archivePath: string;
  archiveStem: string;
  entryPath: string;
  pngRel: string;
  name: string;
  folder: string;
}

/** A real `._xmsh` mesh found in a game archive — metadata only, no `.obj` conversion has
 * happened yet (see `meshObjUrl` for that, done lazily on demand). */
export interface MeshEntry {
  group: string;
  archivePath: string;
  archiveStem: string;
  entryPath: string;
  name: string;
  folder: string;
}

/** A real `._xmac` actor (skeleton + bind-pose mesh + materials for one character/creature),
 * same shape as `MeshEntry` — see `actorObjUrl` for the lazy `.obj` conversion. */
export type ActorEntry = MeshEntry;

/** A real `._xmot` motion clip (one animation, e.g. "Hero_Stand_..._Ambient_Loop..."). Browsing
 * only for now — keyframe playback isn't implemented (see risenlab-presentation-deadline
 * memory for the `.xmot` reverse-engineering status). */
export type MotionEntry = MeshEntry;

/** One bone in a real `._xmac` actor's skeleton — name, parent link, and bind-pose local
 * transform, parsed directly from the file (see `xmac::parse_skeleton` on the Rust side). */
export interface SkeletonNode {
  name: string;
  parentIndex: number | null;
  /** [x, y, z, w] */
  rotation: [number, number, number, number];
  /** [x, y, z] */
  position: [number, number, number];
}

/** One bone's real keyframe tracks from a `._xmot` motion clip — empty arrays mean the clip
 * doesn't animate that bone (not an error). See `xmot::parse_motion` on the Rust side. */
export interface BoneMotion {
  boneName: string;
  /** [x, y, z, time] */
  positionKeys: [number, number, number, number][];
  /** [x, y, z, w, time] */
  rotationKeys: [number, number, number, number, number][];
  /** [x, y, z, time] */
  scaleKeys: [number, number, number, number][];
}

/** A real actor's skinned mesh — positions/normals/UVs/faces plus per-vertex bone weights,
 * parsed directly from `._xmac` bytes (`xmesh_skin::parse_skinned_mesh` on the Rust side).
 * Unlike the `.obj` export (`actorObjUrl`), this carries real skin data so the surface can
 * actually deform with the skeleton instead of just showing a static bind pose. */
/** One material from an actor's own materials section: its name plus the real diffuse/normal
 * texture file names it references (no extension — the game stores them that way). */
export interface SkinnedMeshMaterial {
  name: string;
  diffuse: string | null;
  normal: string | null;
}

export interface SkinnedMeshData {
  positions: [number, number, number][];
  normals: [number, number, number][];
  uvs: [number, number][];
  faces: [number, number, number][];
  /** Parallel to `positions`: up to a few `[boneNodeIndex, weight]` pairs each — indices into
   * the same skeleton node list `actorSkeleton` returns. Empty array = unskinned vertex. */
  skinWeights: [number, number][][];
  /** The actor's real materials, in file order — the index space `faceMaterialIds` uses. */
  materials: SkinnedMeshMaterial[];
  /** Parallel to `faces`: which material each triangle uses. Real actors are multi-material
   * (Wolf = Body + Claws + engine-default), so one texture for the whole mesh is wrong. */
  faceMaterialIds: number[];
}

/** Real texture file names a mesh/actor's own material(s) reference, straight from the game's
 * material data (not a name-matching guess) — see `meshTextureRefs`/`actorTextureRefs`. */
export interface MaterialTextureRefs {
  diffuse: string | null;
  normal: string | null;
}

export interface TextureMeta {
  width: number;
  height: number;
  pixelFormat: string;
  fileSize: number;
}

export interface AppSettings {
  gameExe: string | null;
  outputDir: string;
  patchDir: string;
  reviewHtml: string;
  language: "uk" | "en";
  /** AI provider: "replicate" (default) | "stability". */
  aiProvider?: string | null;
  /** Provider API token — real AI texture enhancement turns on when this is set (the Rust
   * CLI reads it straight from settings.json; empty/absent = local Lanczos fallback). */
  aiApiKey?: string | null;
  /** Replicate model override (`owner/name`); empty = the built-in default upscaler. */
  aiModel?: string | null;
  /** 0.1–0.9 "how much may the AI invent" dial; the AI mode buttons are presets over it. */
  aiCreativity?: number | null;
  /** "✨ Нові текстури" mode: true = the AI fully repaints the texture (high strength, a
   * reimagine prompt) instead of faithfully re-detailing it. */
  aiRegenerate?: boolean | null;
  /** Remote-access tunnel backend: `"cloudflare"` (default) | `"ngrok"`. Added 2026-07-21 —
   * ngrok is the fallback for networks that block Cloudflare Tunnel's registration IPs (a real
   * owner network did — confirmed at the raw TCP level, independent of this app). */
  remoteTunnelProvider?: string | null;
  /** Required when `remoteTunnelProvider` is `"ngrok"` — free account, ngrok.com dashboard;
   * ngrok has required a signed-up authtoken for every tunnel since ~2021, unlike cloudflared. */
  ngrokAuthtoken?: string | null;
}

export interface GameCheckResult {
  root: string;
  archiveCount: number;
  totalBytes: number;
  texturesExtracted: number;
}

export type ReviewStatus = "pending" | "approved" | "rejected";

export interface ReviewItem {
  pngRel: string;
  status: ReviewStatus;
}

export interface AppStats {
  textureTotal: number;
  textureProcessed: number;
  archiveCount: number | null;
  gameArchiveTotalBytes: number | null;
  outputDirSizeBytes: number;
  modelsAvailable: number;
  appVersion: string;
}

/** See `app/src-tauri/src/remote.rs` — a colleague opens `tunnelUrl` with `?token=<token>`
 * appended to reach the app remotely. `tunnelUrl` starts `null` right after starting (cloudflare
 * takes a couple seconds to hand one out) — poll `getRemoteStatus` until it appears. */
export interface RemoteStatus {
  running: boolean;
  port: number | null;
  token: string | null;
  tunnelUrl: string | null;
  cloudflaredAvailable: boolean;
  /** Whether `ngrok.exe` is present, independent of which provider is currently selected. */
  ngrokAvailable: boolean;
  /** Which backend is (or would be) used: `"cloudflare"` | `"ngrok"`. */
  provider: string;
}
