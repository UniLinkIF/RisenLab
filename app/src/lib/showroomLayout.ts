// Pure placement math for the "Вітрина"/Showroom screen (owner request, 2026-07-20: a synthetic
// museum-style hall the app itself lays out — swords/shields on walls, characters standing in
// rows, food/valuables on tables — built from REAL game items/actors, not a reconstruction of an
// actual game location (that would need the level/world format, which this project doesn't parse
// at all). Kept free of THREE/DOM so grid/row/stacking math is unit-testable without a renderer.

export type Vec3 = [number, number, number];

/** One flat grid, either mounted on a vertical WALL (columns run along X, rows run down -Y) or
 * laid on the floor/a table (columns run along X, rows run along +Z, away from the viewer). */
export interface GridSpec {
  count: number;
  columns: number;
  cellSize: number;
  origin: Vec3;
  axis: "wall" | "floor";
}

/** `count` evenly spaced slots along a single line (the "characters standing in a row" case). */
export interface RowSpec {
  count: number;
  spacing: number;
  origin: Vec3;
  /** Which world axis the row runs along — X (side by side) is the natural "standing in a row". */
  axis: "x" | "z";
}

export function gridPositions(spec: GridSpec): Vec3[] {
  const { count, columns, cellSize, origin, axis } = spec;
  const cols = Math.max(1, columns);
  const positions: Vec3[] = [];
  for (let i = 0; i < count; i++) {
    const col = i % cols;
    const row = Math.floor(i / cols);
    if (axis === "wall") {
      // Column across, row DOWN the wall (a shelf/rack reads top-to-bottom).
      positions.push([origin[0] + col * cellSize, origin[1] - row * cellSize, origin[2]]);
    } else {
      // Column across, row AWAY from the viewer (deeper into the room).
      positions.push([origin[0] + col * cellSize, origin[1], origin[2] + row * cellSize]);
    }
  }
  return positions;
}

/** How many rows a grid with this many columns needs — used to size a zone's footprint before
 * stacking the next one after it (see `stackZones`). */
export function gridRowCount(count: number, columns: number): number {
  return Math.max(1, Math.ceil(count / Math.max(1, columns)));
}

export function rowPositions(spec: RowSpec): Vec3[] {
  const { count, spacing, origin, axis } = spec;
  const positions: Vec3[] = [];
  // Centered on the origin, not starting AT it — a row of 1 sits exactly on origin, a row of
  // many is centered around it, so the "front door" of a zone doesn't shift as its count changes.
  const totalSpan = (count - 1) * spacing;
  const start = -totalSpan / 2;
  for (let i = 0; i < count; i++) {
    const offset = start + i * spacing;
    positions.push(axis === "x" ? [origin[0] + offset, origin[1], origin[2]] : [origin[0], origin[1], origin[2] + offset]);
  }
  return positions;
}

/** A zone's world-space depth along Z (how much hall length it needs) — used to stack zones one
 * after another with no overlap, regardless of how many items happen to be in each. */
export interface ZoneFootprint {
  id: string;
  depth: number;
}

/** Stacks zones sequentially along +Z with a fixed gap between them, starting at `startZ`.
 * Returns each zone's own origin Z (the front edge, where its content begins) — a zone with
 * `depth` 0 still gets its own slot, so an empty/skipped zone doesn't collapse into its
 * neighbor. */
export function stackZones(zones: ZoneFootprint[], startZ: number, gap: number): Record<string, number> {
  const origins: Record<string, number> = {};
  let z = startZ;
  for (const zone of zones) {
    origins[zone.id] = z;
    z += zone.depth + gap;
  }
  return origins;
}

/** Uniform scale factor to fit an object's real (loaded) bounding-box size into `targetSize` on
 * its longest axis — game meshes span wildly different native scales (a coin vs. a greatsword vs.
 * a troll), so every showroom slot needs this to look like a consistent display, not a random mix
 * of tiny and giant props. */
export function normalizeScale(boundingSize: Vec3, targetSize: number): number {
  const longest = Math.max(boundingSize[0], boundingSize[1], boundingSize[2]);
  if (!Number.isFinite(longest) || longest <= 0) return 1;
  return targetSize / longest;
}
