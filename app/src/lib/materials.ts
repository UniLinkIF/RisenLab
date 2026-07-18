// Multi-material mesh helpers. Real game meshes are routinely multi-material (the Titan
// weapons pair an axes atlas with a sword-misc atlas; the Wolf actor is Body + Claws + an
// engine-default) — rendering the whole mesh with the first material's texture left every
// other part visibly wrong ("incomplete", per the owner's live testing against Rimy3D, which
// reads the full material list). Pure data logic only, so it's directly unit-testable.

import { findTextureByBaseName } from "./library";

/** Derives the normal-map texture name from a diffuse texture name, mirroring the same
 * real naming convention `batch.rs` uses on the Rust side (`..._Diffuse_01` pairs with
 * `..._Normal_01` across the actual game archives). Returns null when the name has no
 * diffuse marker to replace — not every material follows the convention. */
export function deriveNormalName(diffuseName: string): string | null {
  for (const [from, to] of [
    ["_Diffuse_", "_Normal_"],
    ["_diffuse_", "_normal_"],
    ["Diffuse", "Normal"],
  ] as const) {
    if (diffuseName.includes(from)) return diffuseName.replace(from, to);
  }
  return null;
}

/** Same convention as `deriveNormalName`, for the `..._Specular_01` textures the real game
 * archives also ship alongside diffuse/normal (confirmed real name: e.g.
 * `ItWpn_Axes_01_Specular_01`). Used to turn the flat, one-size-fits-all viewer roughness into
 * something that at least varies per real material — see `lib/roughness.ts` for the actual
 * specular→roughness conversion and its accuracy caveat. */
export function deriveSpecularName(diffuseName: string): string | null {
  for (const [from, to] of [
    ["_Diffuse_", "_Specular_"],
    ["_diffuse_", "_specular_"],
    ["Diffuse", "Specular"],
  ] as const) {
    if (diffuseName.includes(from)) return diffuseName.replace(from, to);
  }
  return null;
}

/** Finds the real library texture for a base name the way the game's own data references it:
 * exact base-name match first (with `findTextureByBaseName`'s `_Ghost` fallback), then with the
 * game's `_S1` (highest-detail stage) suffix appended — actor material names reference e.g.
 * "Ani_Monster_Wolf_Body_01_Diffuse" while the real texture file is "..._Diffuse_S1". */
export function findTextureEntryForBaseName<T extends { name: string }>(entries: T[], baseName: string): T | null {
  const exact = findTextureByBaseName(entries, baseName) ?? findTextureByBaseName(entries, `${baseName}_S1`);
  if (exact) return exact;
  // Last resort — infix drift between actor materials and real files (real case: the Ogre's
  // materials reference "Ani_Monster_Oger_Cloth_Diffuse_S1" while the game ships
  // "Ani_Hero_Monster_Oger_Cloth_Diffuse_S1", so the belt fell back to the body texture):
  // match by the distinctive TAIL of the name (reference minus its first token). Only
  // accepted when long enough to be unambiguous and exactly one candidate matches — a wrong
  // texture is worse than the fallback.
  const stripExt = (n: string) => {
    const dot = n.lastIndexOf(".");
    return (dot === -1 ? n : n.slice(0, dot)).toLowerCase();
  };
  const target = stripExt(baseName);
  const tail = target.slice(target.indexOf("_") + 1);
  if (tail.length < 12 || tail === target) return null;
  const candidates = entries.filter((e) => {
    const n = stripExt(e.name);
    return n.endsWith(tail) || n.endsWith(`${tail}_s1`);
  });
  return candidates.length === 1 ? candidates[0] : null;
}

export interface MaterialGroup {
  /** Offset into the flattened index buffer (3 entries per face). */
  start: number;
  /** Number of index entries (3 per face). */
  count: number;
  materialId: number;
}

/** Reorders faces into contiguous runs per material id and returns the flattened index buffer
 * plus one `{start, count, materialId}` group per distinct id — exactly the shape
 * `THREE.BufferGeometry.addGroup` + a material array need. Face order within a material is
 * preserved (stable), and faces beyond the ids array (defensive) fall back to material 0. */
export function groupFacesByMaterial(
  faces: [number, number, number][],
  faceMaterialIds: number[],
): { index: number[]; groups: MaterialGroup[] } {
  const byMaterial = new Map<number, [number, number, number][]>();
  faces.forEach((face, i) => {
    const id = faceMaterialIds[i] ?? 0;
    const bucket = byMaterial.get(id);
    if (bucket) bucket.push(face);
    else byMaterial.set(id, [face]);
  });

  const index: number[] = [];
  const groups: MaterialGroup[] = [];
  for (const [materialId, bucket] of [...byMaterial.entries()].sort((a, b) => a[0] - b[0])) {
    const start = index.length;
    for (const [a, b, c] of bucket) index.push(a, b, c);
    groups.push({ start, count: index.length - start, materialId });
  }
  return { index, groups };
}
