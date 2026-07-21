// Manual item -> scenario associations, persisted in localStorage — same pattern as
// actorOrientation.ts (an injectable KeyValueStorage so this is testable without a DOM).
//
// WHY this exists: `matchScenariosForItemName` (scenarios.ts) only catches an item↔scenario
// pairing when they share a real name token (e.g. "Item_Flute" -> "Play Flute"). Real items
// exist where the game's own naming doesn't share a word with the scenario action at all (a
// lute-like instrument item whose scenario is literally named "PlayGuitar", say) — the automatic
// match misses those on purpose (matching on nothing would just be noise). This lets the owner
// manually confirm a pairing once, the same way actorOrientation.ts lets them confirm a
// front/back flip once instead of re-guessing every session.

export interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function defaultStorage(): KeyValueStorage | null {
  return typeof localStorage === "undefined" ? null : localStorage;
}

const STORAGE_KEY = "risenlab.itemScenarioOverrides.v1";

function readStore(storage: KeyValueStorage | null): Record<string, string[]> {
  if (!storage) return {};
  try {
    const raw = storage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Record<string, string[]>) : {};
  } catch {
    return {};
  }
}

function writeStore(storage: KeyValueStorage | null, store: Record<string, string[]>): void {
  if (!storage) return;
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify(store));
  } catch {
    // Best-effort only, same rationale as actorOrientation.ts — a full/unavailable storage
    // shouldn't crash the viewer, it just means the association won't survive a reload.
  }
}

/** Scenario IDs manually added for this item (by its own real mesh `name`), on top of whatever
 * `matchScenariosForItemName` already finds automatically. */
export function getManualScenarioIds(itemName: string, storage = defaultStorage()): string[] {
  return readStore(storage)[itemName] ?? [];
}

export function addManualScenario(itemName: string, scenarioId: string, storage = defaultStorage()): void {
  const store = readStore(storage);
  const existing = store[itemName] ?? [];
  if (!existing.includes(scenarioId)) {
    store[itemName] = [...existing, scenarioId];
    writeStore(storage, store);
  }
}

export function removeManualScenario(itemName: string, scenarioId: string, storage = defaultStorage()): void {
  const store = readStore(storage);
  const existing = store[itemName] ?? [];
  if (existing.includes(scenarioId)) {
    store[itemName] = existing.filter((id) => id !== scenarioId);
    writeStore(storage, store);
  }
}
