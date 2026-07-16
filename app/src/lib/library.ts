// Pure data-shaping helpers for the Library/Search screens — no Tauri, no DOM, so every
// function here is directly unit-testable.

import type { LibraryEntry, ReviewStatus } from "./types";

export interface TreeNode {
  key: string;
  label: string;
  depth: number;
  count: number;
}

/** The minimal shape the folder-tree/search helpers below need — satisfied by both
 * `LibraryEntry` (textures) and `MeshEntry` (meshes), so the same Library UI components
 * (FolderTree, search) work for either without duplicating this logic. */
export interface TreeEntry {
  group: string;
  archiveStem: string;
  folder: string;
  name: string;
}

/** Finds a real texture whose base name (ignoring extension) matches a material's referenced
 * texture file name — used to auto-apply the diffuse/normal a mesh/actor's own material
 * already points at (see `meshTextureRefs`/`actorTextureRefs`) instead of asking the user to
 * pick one by hand. The reference name comes from the game's original dev-time material data
 * (e.g. "..._Diffuse_01.tga") while the real library entry is the runtime `._ximg` (e.g.
 * "..._Diffuse_01._ximg") — different extensions, same base name, so that's what's compared.
 *
 * Falls back to the base name for a "_Ghost" (spectral/translucent item variant) reference —
 * real bug found live: "It_Helmet_TitanLord_Ghost"'s material is named
 * "..._Diffuse_S1_Ghost", but no texture file has that exact name; only the non-Ghost base
 * item's texture exists (the real game tints it via a material property at runtime instead of
 * baking a separate image). Same fallback as `batch::embed_real_texture_paths` on the Rust
 * side — keep both in sync if this list grows. */
export function findTextureByBaseName<T extends { name: string }>(entries: T[], refName: string): T | null {
  const stripExt = (n: string) => {
    const dot = n.lastIndexOf(".");
    return (dot === -1 ? n : n.slice(0, dot)).toLowerCase();
  };
  const target = stripExt(refName);
  const exact = entries.find((e) => stripExt(e.name) === target);
  if (exact) return exact;
  if (target.endsWith("_ghost")) {
    return findTextureByBaseName(entries, target.slice(0, -"_ghost".length));
  }
  return null;
}

/** Splits a folder path into its segments, tolerating a leading/trailing "/" (real archive
 * entry paths come back from the pak reader with a leading slash) and empty folders. */
function folderSegments(folder: string): string[] {
  return folder
    .split("/")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** The full group/archive/folder key an entry belongs to in the sidebar tree — preserves
 * every real folder level (e.g. "Animation/Monster"), not just the first segment, so nested
 * folders stay browsable instead of collapsing into their parent. */
export function entryTreeKey(e: TreeEntry): string {
  const segments = folderSegments(e.folder);
  const folderPart = segments.length > 0 ? segments.join("/") : "(root)";
  return `${e.group}/${e.archiveStem}/${folderPart}`;
}

/** Builds a flattened, depth-annotated tree (group -> archive -> folder -> subfolder -> ...)
 * with per-node counts, ready to render as an indented list matching the approved design's
 * folder tree. Recurses to whatever real folder depth the archive actually has. */
export function buildFolderTree(entries: TreeEntry[]): TreeNode[] {
  const counts = new Map<string, number>();
  const bump = (key: string) => counts.set(key, (counts.get(key) ?? 0) + 1);
  for (const e of entries) {
    bump(e.group);
    bump(`${e.group}/${e.archiveStem}`);
    const segments = folderSegments(e.folder);
    const folderKeyParts = segments.length > 0 ? segments : ["(root)"];
    let prefix = `${e.group}/${e.archiveStem}`;
    for (const part of folderKeyParts) {
      prefix = `${prefix}/${part}`;
      bump(prefix);
    }
  }

  const nodes: TreeNode[] = [];
  const groups = [...new Set(entries.map((e) => e.group))].sort();
  for (const g of groups) {
    nodes.push({ key: g, label: g, depth: 0, count: counts.get(g) ?? 0 });
    const inGroup = entries.filter((e) => e.group === g);
    const archives = [...new Set(inGroup.map((e) => e.archiveStem))].sort();
    for (const a of archives) {
      const gaKey = `${g}/${a}`;
      nodes.push({ key: gaKey, label: a, depth: 1, count: counts.get(gaKey) ?? 0 });
      const inArchive = inGroup.filter((e) => e.archiveStem === a);
      addFolderNodes(nodes, counts, inArchive, gaKey, 2);
    }
  }
  return nodes;
}

/** Recursively emits one tree node per distinct folder segment at `depth`, then recurses into
 * each one's children for the next depth level — this is what lets e.g. "Animation" expand
 * into its own "Monster" child instead of every nested folder merging into one bucket. */
function addFolderNodes<T extends TreeEntry>(
  nodes: TreeNode[],
  counts: Map<string, number>,
  entries: T[],
  parentKey: string,
  depth: number,
): void {
  const withRemainder = entries
    .map((e) => ({ e, remainder: folderSegments(e.folder).slice(depth - 2) }))
    .filter((x) => x.remainder.length > 0);
  const segmentsAtThisDepth = [...new Set(withRemainder.map((x) => x.remainder[0]))].sort();
  for (const seg of segmentsAtThisDepth) {
    const key = `${parentKey}/${seg}`;
    nodes.push({ key, label: seg, depth, count: counts.get(key) ?? 0 });
    const inSeg = withRemainder.filter((x) => x.remainder[0] === seg).map((x) => x.e);
    addFolderNodes(nodes, counts, inSeg, key, depth + 1);
  }
}

/** Entries whose tree key is `key` or nested under it. `null`/`""` means "everything". */
export function filterByTreeKey<T extends TreeEntry>(entries: T[], key: string | null): T[] {
  if (!key) return entries;
  return entries.filter((e) => {
    const k = entryTreeKey(e);
    return k === key || k.startsWith(`${key}/`);
  });
}

// Real top-level folders in the game's images.pak that are flat 2D artwork never mapped onto
// a 3D mesh/material — interface chrome, editor-only icons, debug/test assets, baked lighting
// maps. Folders NOT listed here (Animation, Level, Sky, Special, Speedtree, ...) hold real
// diffuse/normal maps applied to in-game models, even when their own name looks generic (e.g.
// weapon diffuse maps live under "Special") — so this is deliberately a narrow, conservative
// blocklist rather than a guess at what "looks like a photo".
const FLAT_2D_TOP_FOLDERS = new Set(["gui", "editsupporter", "achievements", "lightmaps", "nomip", "testkram"]);

/** True for textures that are flat 2D artwork never applied to a 3D model (UI chrome, editor
 * icons, baked lightmaps, debug assets) — the "hide 2D textures" toggle in Library. */
export function isFlat2DOnly(entry: TreeEntry): boolean {
  const top = folderSegments(entry.folder)[0]?.toLowerCase();
  return top !== undefined && FLAT_2D_TOP_FOLDERS.has(top);
}

/** Case-insensitive substring search across name/folder/archive — the whole point of this
 * being generic is that it's also Library's one true search box (see [[risenlab-ui-vision]]:
 * no separate search screen), reused as-is for the Models screen's mesh search. */
export function filterEntries<T extends TreeEntry>(entries: T[], query: string): T[] {
  const q = query.trim().toLowerCase();
  if (!q) return entries;
  return entries.filter(
    (e) =>
      e.name.toLowerCase().includes(q) ||
      e.folder.toLowerCase().includes(q) ||
      e.archiveStem.toLowerCase().includes(q),
  );
}

const BYTE_UNITS = ["KB", "MB", "GB", "TB"];

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  let value = n / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < BYTE_UNITS.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  const decimals = value < 10 ? 1 : 0;
  return `${value.toFixed(decimals)} ${BYTE_UNITS[unitIndex]}`;
}

export interface StatusBadge {
  label: string;
  background: string;
}

/** Grid-card badge for a texture's processing status — "already handled" vs. "still to do",
 * per the owner's request for visible per-file progress tracking. `undefined`/no entry means
 * untouched since extraction (no badge). */
export function badgeForStatus(status: ReviewStatus | undefined, lang: "uk" | "en"): StatusBadge | null {
  if (status === "approved") {
    return { label: lang === "uk" ? "ГОТОВО" : "DONE", background: "var(--green)" };
  }
  if (status === "pending") {
    return { label: "AI", background: "var(--accent)" };
  }
  return null;
}

/** Counts how many library entries have a recorded status (approved or pending review) —
 * the "X out of Y processed" summary. */
export function countProcessed(entries: LibraryEntry[], status: Map<string, ReviewStatus>): number {
  return entries.reduce((count, e) => (status.has(e.pngRel) ? count + 1 : count), 0);
}
