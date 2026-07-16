// Per-actor front/back orientation override for the Animations skeleton viewer, persisted in
// localStorage.
//
// WHY this exists instead of a real algorithm: the game data has a genuine per-asset authoring
// inconsistency (some actors' raw skeleton data needs a 180°-yaw mirror to face the right way
// relative to their own bind-pose mesh, some don't — see risenlab-animation-research memory).
// A from-scratch geometric auto-detector was tried here (score each of the two candidate
// orientations by average nearest-mesh-vertex distance per bone) and validated against the two
// actors whose correct answer is already confirmed by a human watching it live: it picked
// "mirror" for BOTH Wolf (confirmed-correct: identity) and Pig (confirmed-correct: mirror) — a
// wrong answer for Wolf, so that approach does not reliably discriminate the two candidates and
// was not shipped. Until a real discriminating signal is found, this instead remembers whichever
// answer a human already confirmed by eye, per actor — so each actor only ever needs correcting
// once (by toggling a checkbox), not "the same one method applied to everyone" as before this
// file existed.
//
// Skeleton and mesh are stored as two INDEPENDENT flags, not one "flip everything" toggle:
// mirroring both by the same amount can never change their alignment relative to EACH OTHER
// (it's the same rigid rotation applied to both, only changing which way the whole rig faces the
// camera) — see SkeletonAnimationViewer's `Props` doc comment. Independent flags are a real
// manual realignment tool for the separate case where an actor's mesh/skin data and its skeleton
// data aren't in the same coordinate convention.

export interface ActorOrientation {
  mirrorSkeleton: boolean;
  mirrorMesh: boolean;
}

const IDENTITY: ActorOrientation = { mirrorSkeleton: false, mirrorMesh: false };

/** The minimal storage shape this needs — satisfied by the real `localStorage` and by a plain
 * in-memory fake in tests, so persistence logic is testable without a DOM. */
export interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function defaultStorage(): KeyValueStorage | null {
  return typeof localStorage === "undefined" ? null : localStorage;
}

const STORAGE_KEY = "risenlab.actorOrientationOverrides.v2";

// Seeded from real, owner-confirmed live sessions (risenlab-animation-research memory,
// 2026-07-15 night): Wolf plays correctly under identity; Pig plays backwards under identity and
// needs mirroring — both skeleton and mesh together, matching the single "mirrored" toggle that
// existed before this file split them (same net visual effect). Keyed the same way
// `orientationKey` builds keys below.
const SEEDED_DEFAULTS: Record<string, ActorOrientation> = {
  "animations.pak::/_emfx36/Monster/Bodys/Ani_Wolf_Monster_Wolf._xmac": IDENTITY,
  "animations.pak::/_emfx36/Monster/Bodys/Ani_Pig_Monster_Pig._xmac": { mirrorSkeleton: true, mirrorMesh: true },
};

/** Only the archive's own file name matters for the key — the same real archive can be reached
 * via different absolute paths on different machines (the game path is user-chosen, see
 * `gameExe` in settings). */
export function orientationKey(archivePath: string, entryPath: string): string {
  const archiveName = archivePath.split(/[\\/]/).pop() ?? archivePath;
  return `${archiveName}::${entryPath}`;
}

function readStore(storage: KeyValueStorage | null): Record<string, ActorOrientation> {
  if (!storage) return {};
  try {
    const raw = storage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Record<string, ActorOrientation>) : {};
  } catch {
    return {};
  }
}

function writeStore(storage: KeyValueStorage | null, store: Record<string, ActorOrientation>): void {
  if (!storage) return;
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify(store));
  } catch {
    // Best-effort only — a full/unavailable storage shouldn't crash the viewer, it just means
    // the override won't survive a reload.
  }
}

/** The orientation to use for this actor: a value the user has explicitly confirmed for it
 * (persisted across sessions), else a real owner-confirmed seed for the two actors already
 * tested live, else identity (matches the game's raw convention for most actors). */
export function getActorOrientation(archivePath: string, entryPath: string, storage = defaultStorage()): ActorOrientation {
  const key = orientationKey(archivePath, entryPath);
  const store = readStore(storage);
  if (key in store) return store[key];
  return SEEDED_DEFAULTS[key] ?? IDENTITY;
}

export function setActorOrientation(
  archivePath: string,
  entryPath: string,
  orientation: ActorOrientation,
  storage = defaultStorage(),
): void {
  const key = orientationKey(archivePath, entryPath);
  const store = readStore(storage);
  store[key] = orientation;
  writeStore(storage, store);
}
