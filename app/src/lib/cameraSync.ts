/** Shared camera state for two side-by-side 3D viewers (owner request, 2026-07-19: rotating/
 * zooming one comparison panel should move the other identically — much easier to spot a real
 * texture/pose difference than eyeballing two independently-orbited cameras). A plain mutable
 * ref rather than React state: camera drags fire dozens of times a second and must never trigger
 * a React re-render — each viewer polls this once per animation frame instead. `rev` is a
 * monotonic timestamp (not a per-instance counter) so whichever panel wrote most recently always
 * wins, with no risk of two panels' independent counters colliding. */
export interface CameraSyncState {
  rev: number;
  position: [number, number, number];
  target: [number, number, number];
}

export type CameraSyncRef = { current: CameraSyncState | null };
