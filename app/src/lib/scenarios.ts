import type { MotionEntry } from "./types";

/** Real folder every clip this module cares about lives in — confirmed against the owner's
 * connected game (2026-07-21): 488 real `Hero_*` clips, EVERY one exactly 14 `_`-separated
 * tokens once the trailing `._xmot` is stripped. Other `Hero_*` folders (Fight/Bossfight/
 * Dialog/Ambient) use the same 14-token shape but different field meanings (movement/combat
 * clips, not "sit down and use something") — restricting to this folder keeps the grammar below
 * actually valid for everything it's applied to, not just the flute example it was built from. */
const INTERACTS_FOLDER = "_emfx36/Humans/Animations/Interacts";

/** One real clip's name, decoded by FIXED TOKEN INDEX (verified against all 488 real Interacts
 * clips, not inferred from a handful of examples) —
 * `Hero_<fromState>_<sub>_<heldItem>_P#_<action>_<phase>_<O|N>_<direction>_<v1>_%_<v2>_P#_<frame>`.
 * `action` is either a real action verb ("PlayFlute", "EnjoyPotion") or another real `fromState`
 * name, in which case the clip IS the transition between those two states, not an activity. */
interface ParsedInteractClip {
  entry: MotionEntry;
  fromState: string;
  heldItem: string | null;
  action: string;
  phase: string;
  isOverlay: boolean;
  direction: string;
}

function parseInteractClip(entry: MotionEntry): ParsedInteractClip | null {
  const stem = entry.name.replace(/\._xmot$/i, "");
  const tokens = stem.split("_");
  if (tokens.length !== 14 || tokens[0] !== "Hero") return null;
  return {
    entry,
    fromState: tokens[1],
    heldItem: tokens[3] === "None" ? null : tokens[3],
    action: tokens[5],
    phase: tokens[6],
    isOverlay: tokens[7] === "O",
    direction: tokens[8],
  };
}

/** When several real clips exist for the same (fromState, item, action, phase) — direction
 * variants (Fwd/Left/Right/Back) and alternate idle-fidget numbers — picks one deterministically:
 * prefer the plain forward, non-overlay, "00" variant real content is actually authored around
 * (every example seen has exactly this one), falling back to whichever sorts first so the choice
 * is at least stable across renders instead of arbitrary. */
function pickCanonical(clips: ParsedInteractClip[]): ParsedInteractClip {
  const preferred = clips.find((c) => !c.isOverlay && c.direction === "Fwd" && c.entry.name.includes("_00_%_00_"));
  if (preferred) return preferred;
  const nonOverlayFwd = clips.find((c) => !c.isOverlay && c.direction === "Fwd");
  if (nonOverlayFwd) return nonOverlayFwd;
  return [...clips].sort((a, b) => a.entry.name.localeCompare(b.entry.name))[0];
}

/** Real actions that are just a hand-pose OVERLAY for holding an item (upper-body-only, layered
 * on top of a base pose) — not a standalone visual scenario on their own. */
const EXCLUDED_ACTIONS = new Set(["HoldRight", "HoldLeft"]);

/** "PlayFlute" -> "Play Flute", "DigGround" -> "Dig Ground" — no translation dictionary (101
 * distinct real actions is too many to hand-translate), just readable spacing. */
function prettifyAction(action: string): string {
  return action.replace(/([a-z0-9])([A-Z])/g, "$1 $2");
}

export interface ScenarioDef {
  id: string;
  label: string;
  /** Real clip names in play order, ready for `motionTracks` lookup by exact name match
   * against the caller's own `motions` list. */
  clips: { label: string; name: string; sustain?: boolean; advanceLabel?: string }[];
}

/** Derives every real "use an inventory-style item / perform an action" scenario from the
 * Hero's own real `Interacts` motion clips — see risenlab-inventory-scenario-idea memory
 * (2026-07-21): the owner's flute example, generalized. Mechanical, not curated: any real
 * (fromState, action) pair with at least a Begin or Loop clip becomes a selectable scenario,
 * except pure state-transitions (action names another real fromState) and hand-pose overlays.
 *
 * Owner correction (2026-07-21, same morning): only ONE dismiss point per scenario, matching
 * how the real game actually works — "закінчити" (finish) auto-plays End then the exit
 * transition back to Stand in one go, not a separate second "встати" (stand up) step. */
export function deriveScenarios(motions: MotionEntry[]): ScenarioDef[] {
  const parsed = motions
    .filter((m) => m.folder === INTERACTS_FOLDER && m.name.startsWith("Hero_"))
    .map(parseInteractClip)
    .filter((p): p is ParsedInteractClip => p !== null);

  const knownStates = new Set(parsed.map((p) => p.fromState));

  // Transitions: `action` names another real fromState — e.g. fromState="Stand", action=
  // "SitGround", phase="Begin" is the "sit down on the ground" clip. Keyed "From->To".
  const transitions = new Map<string, ParsedInteractClip[]>();
  // Activities: grouped by (fromState, item, action) -> clips for every phase found.
  const activities = new Map<string, ParsedInteractClip[]>();

  for (const p of parsed) {
    if (knownStates.has(p.action)) {
      const key = `${p.fromState}->${p.action}`;
      (transitions.get(key) ?? transitions.set(key, []).get(key)!).push(p);
    } else if (!EXCLUDED_ACTIONS.has(p.action)) {
      const key = `${p.fromState}::${p.heldItem ?? ""}::${p.action}`;
      (activities.get(key) ?? activities.set(key, []).get(key)!).push(p);
    }
  }

  function transitionClip(from: string, to: string): ParsedInteractClip | null {
    const clips = transitions.get(`${from}->${to}`);
    return clips && clips.length > 0 ? pickCanonical(clips.filter((c) => c.phase === "Begin")) : null;
  }

  const scenarios: ScenarioDef[] = [];
  for (const [key, clips] of activities) {
    // The held-item token (middle segment) only ever appears on the HoldRight overlay clips
    // this module excludes, not on the base activity clips themselves — kept in the map key for
    // correctness (in case that ever changes) but unused here.
    const [fromState, , action] = key.split("::");
    const byPhase = new Map<string, ParsedInteractClip>();
    for (const phase of ["Begin", "Loop", "End"]) {
      const candidates = clips.filter((c) => c.phase === phase);
      if (candidates.length > 0) byPhase.set(phase, pickCanonical(candidates));
    }
    const begin = byPhase.get("Begin");
    const loop = byPhase.get("Loop");
    const end = byPhase.get("End");
    if (!begin && !loop) continue; // nothing to actually play

    const steps: ScenarioDef["clips"] = [];
    const isStanding = fromState === "Stand";
    if (!isStanding) {
      const enter = transitionClip("Stand", fromState);
      if (enter) steps.push({ label: `Переходить у стан «${prettifyAction(fromState)}»`, name: enter.entry.name });
    }
    if (begin) steps.push({ label: `Починає: ${prettifyAction(action)}`, name: begin.entry.name });
    if (loop) steps.push({ label: prettifyAction(action), name: loop.entry.name, sustain: true, advanceLabel: "⏹ Закінчити" });
    if (end) steps.push({ label: `Закінчує: ${prettifyAction(action)}`, name: end.entry.name });
    if (!isStanding) {
      const exit = transitionClip(fromState, "Stand");
      if (exit) steps.push({ label: "Встає", name: exit.entry.name });
    }
    if (steps.length === 0) continue;

    const label = isStanding ? `🎭 ${prettifyAction(action)}` : `🎭 ${prettifyAction(action)} (${prettifyAction(fromState)})`;
    scenarios.push({ id: key, label, clips: steps });
  }

  return scenarios.sort((a, b) => a.label.localeCompare(b.label));
}
