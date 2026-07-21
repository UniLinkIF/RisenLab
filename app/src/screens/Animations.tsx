import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type { Lang } from "../lib/i18n";
import type { ActorEntry, BoneMotion, LibraryEntry, MotionEntry, SkeletonNode, SkinnedMeshData } from "../lib/types";
import type { CameraSyncState } from "../lib/cameraSync";
import { actorObjUrl, actorSkeleton, actorSkinnedMesh, actorTextureRefs, exportDoubleRateMotionPatch, exportMotionPatch, exportMotionPatchBatch, listActors, listLibrary, listMotions, motionTracks, readEditedDataUrl, readTextureDataUrl, regenerateTexture } from "../lib/api";
import { buildFolderTree, filterByTreeKey, filterEntries, findTextureByBaseName } from "../lib/library";
import { findTextureEntryForBaseName } from "../lib/materials";
import { getActorOrientation, setActorOrientation } from "../lib/actorOrientation";
import { MOTION_CATEGORIES, motionCategory, type MotionCategory } from "../lib/motionCategory";
import FolderTree from "../components/FolderTree";
import Model3DViewer, { type ViewMode } from "../components/Model3DViewer";
import SkeletonAnimationViewer, { motionDuration } from "../components/SkeletonAnimationViewer";
import ScenarioPlayer, { type ScenarioStepTracks } from "../components/ScenarioPlayer";
import SearchableList from "../components/SearchableList";
import { deriveScenarios, type ScenarioDef } from "../lib/scenarios";

interface Props {
  lang: Lang;
}

/** One row of the right-hand control sidebar: a label plus a compact strength picker (0 / mid /
 * strong, or whatever `options` says). Used for jitter cleanup and the three animation-quality
 * transforms — same visual pattern, four times, so it's a component instead of four copies. */
function QualityToggle({
  label,
  title,
  value,
  onChange,
  options,
  disabled,
}: {
  label: string;
  title?: string;
  value: number;
  onChange: (v: number) => void;
  options: [number, string][];
  disabled?: boolean;
}) {
  return (
    <div style={{ marginBottom: 10 }}>
      <div title={title} style={{ font: "600 10.5px system-ui", color: "var(--text-faint)", marginBottom: 4, cursor: title ? "help" : undefined }}>
        {label}
      </div>
      <div style={{ display: "flex", gap: 4 }}>
        {options.map(([v, text]) => (
          <button
            key={v}
            onClick={() => onChange(v)}
            disabled={disabled}
            style={{
              flex: 1,
              padding: "5px 4px",
              borderRadius: 10,
              background: value === v ? "var(--accent)" : "var(--bg2)",
              border: `1px solid ${value === v ? "var(--accent)" : "var(--border)"}`,
              font: "600 10.5px system-ui",
              color: value === v ? "#fff" : "var(--text-dim)",
              cursor: disabled ? "wait" : "pointer",
            }}
          >
            {text}
          </button>
        ))}
      </div>
    </div>
  );
}

const sidebarSectionTitle: CSSProperties = {
  font: "600 10px system-ui",
  letterSpacing: ".06em",
  textTransform: "uppercase",
  color: "var(--text-faint)",
  margin: "14px 0 8px",
};
const sidebarDivider: CSSProperties = { height: 1, background: "var(--border)", margin: "12px 0" };

/** Actor filenames look like "Ani_Wolf_Monster_Wolf._xmac" or "Ani_Hero_Head_Player._xmac";
 * motion clips are named "Wolf_Stand_..." / "Hero_Stand_...". There's no exact ID linking the
 * two real formats (see risenlab-presentation-deadline memory), so this pulls a best-guess
 * character token out of the actor name to pre-filter the motion list — a starting point the
 * user can always broaden by editing the search box themselves, not a guaranteed match.
 *
 * Three real actor-name prefixes, three different guessing rules:
 * - "Ani_" (real characters, e.g. "Ani_Wolf_Monster_Wolf") — the FIRST real token ("Wolf")
 *   names the creature, matching that creature's own real folder/clip-name convention.
 * - "Object_" (animated interactables, e.g. "Object_Interact_Animated_Button") — these have no
 *   skeleton animation of their own; what the player sees is really a HERO animation of using
 *   the object, named after the object's own distinguishing word, not the object's file name
 *   (confirmed on real data: "Object_Interact_Animated_Button" ↔
 *   "Hero_Stand_None_None_P0_Button_PushIn..."). The LAST token in the file name is that
 *   distinguishing word for every real one of these (Winch/Waterpipe/GrindStone/Cupboard/...).
 * - "It_" (weapon/item props, e.g. "It_Wpn_Crossbow_War") — no dedicated animation of their own
 *   at all (rigid props driven by the wielder), and their first token is short enough (e.g.
 *   "It") to substring-match huge swaths of unrelated real clips (confirmed on real data: "It"
 *   matched 4865 motions across Titan/Ogre/Lizard/Goblin). Guessing nothing (empty query =
 *   browse everything) beats guessing wrong here. */
function guessMotionQuery(actorName: string): string {
  const stem = actorName.replace(/\._xmac$/i, "");
  const tokens = stem.split("_").filter(Boolean);
  if (actorName.startsWith("Ani_")) return tokens.filter((t) => t !== "Ani")[0] ?? "";
  if (actorName.startsWith("Object_")) return tokens[tokens.length - 1] ?? "";
  return "";
}

export default function Animations({ lang }: Props) {
  const [actors, setActors] = useState<ActorEntry[]>([]);
  const [motions, setMotions] = useState<MotionEntry[]>([]);
  const [textures, setTextures] = useState<LibraryEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  const [actorTreeKey, setActorTreeKey] = useState<string | null>(null);
  const [actorQuery, setActorQuery] = useState("");
  const [selectedActor, setSelectedActor] = useState<ActorEntry | null>(null);
  const [objUrl, setObjUrl] = useState<string | null>(null);
  const [objError, setObjError] = useState<string | null>(null);
  const [objLoading, setObjLoading] = useState(false);
  const [diffuseUrl, setDiffuseUrl] = useState<string | null>(null);
  const [normalUrl, setNormalUrl] = useState<string | null>(null);

  const [motionQuery, setMotionQuery] = useState("");
  const [motionCategoryFilter, setMotionCategoryFilter] = useState<MotionCategory>("all");
  const [selectedMotion, setSelectedMotion] = useState<MotionEntry | null>(null);
  const [selectedScenario, setSelectedScenario] = useState<ScenarioDef | null>(null);
  const [scenarioSteps, setScenarioSteps] = useState<ScenarioStepTracks[] | null>(null);
  const [scenarioError, setScenarioError] = useState<string | null>(null);

  const [mode, setMode] = useState<ViewMode>("textured");

  const [skeletonNodes, setSkeletonNodes] = useState<SkeletonNode[]>([]);
  const [skeletonError, setSkeletonError] = useState<string | null>(null);
  const [skinnedMeshData, setSkinnedMeshData] = useState<SkinnedMeshData | null>(null);

  // Texture-enhancement state (the actor-side counterpart of Models.tsx's "Regenerate"):
  // `enhancedRels` = pngRels that got a fresh `edited/` variant this session (only those are
  // safe to read back — readEditedDataUrl on a texture that was never regenerated 404s, which
  // the viewers would render as a loud magenta error), `showEnhanced` = which variant the
  // per-material resolver serves.
  const [enhancing, setEnhancing] = useState(false);
  const [showEnhanced, setShowEnhanced] = useState(false);
  const [enhancedRels, setEnhancedRels] = useState<Set<string>>(new Set());
  // 2-object original/enhanced TEXTURE comparison (owner request, 2026-07-19 — distinct from
  // the motion `sideBySide` below, which compares POSES not textures). Mutually exclusive with
  // motion side-by-side so the viewer never has to render a 4-way split.
  const [textureSideBySide, setTextureSideBySide] = useState(false);
  // Shared camera for whichever 2-panel comparison is active (owner request, 2026-07-19:
  // orbiting/zooming one panel should move the other identically) — a plain ref, not React
  // state, see lib/cameraSync.ts. Reset on actor change so a leftover framing sized for a
  // different model's bounding box never gets applied to a freshly mounted pair.
  const cameraSyncRef = useRef<CameraSyncState | null>(null);
  useEffect(() => {
    cameraSyncRef.current = null;
  }, [selectedActor?.entryPath]);
  const [motionTracksData, setMotionTracksData] = useState<BoneMotion[] | null>(null);
  const [tracksLoading, setTracksLoading] = useState(false);
  // Jitter-cleanup preview strength (0 = original clip; the filtering itself runs in Rust —
  // `xmot::smooth_tracks`, the same code `smooth-motion` writes real files with — so what
  // this previews is exactly what an export would contain).
  const [smoothStrength, setSmoothStrength] = useState(0);
  // The three "animation quality" transforms (owner request, 2026-07-18): amplitude boost
  // ("💪 Виразність" — spine/arms/head only, never legs/root), secondary motion ("🌊 Вторинний
  // рух" — delayed follow-through for tails/cloth/ears/hair/belts only), and attack retiming
  // ("⚡ Різкість ударів" — slow windup, sharp strike; safe on any clip, most useful on combat
  // ones). All run in Rust (`xmot::stylize_tracks`) — same code the patch export writes.
  const [expressiveness, setExpressiveness] = useState(0);
  const [secondaryMotion, setSecondaryMotion] = useState(0);
  const [sharpness, setSharpness] = useState(0);
  // "🎬 60fps" — PREVIEW ONLY (`xmot::resample_double_rate`): doubles key rate for smoother
  // playback, but changes key counts, which can't be written back into a real .xmot file in
  // place. Deliberately excluded from `styleActive`, which gates the patch-export buttons —
  // only `previewActive` (A/B, side-by-side) includes it.
  const [doubleRate, setDoubleRate] = useState(false);
  const styleActive = smoothStrength > 0 || expressiveness > 0 || secondaryMotion > 0 || sharpness > 0;
  const previewActive = styleActive || doubleRate;
  // A/B compare (owner request): original tracks are always kept alongside the smoothed set,
  // so flipping between them is instant — same clip, same viewer, only the keyframes differ.
  const [originalTracks, setOriginalTracks] = useState<BoneMotion[] | null>(null);
  const [abOriginal, setAbOriginal] = useState(false);
  // Side-by-side compare (owner request): both animations playing at once — left original,
  // right smoothed. Mounted together so their clocks start in sync; play/pause is shared.
  const [sideBySide, setSideBySide] = useState(false);
  const [exportingPatch, setExportingPatch] = useState(false);
  const [patchMessage, setPatchMessage] = useState<string | null>(null);

  // Batch export: every clip the motion list currently shows (creature pre-filter + search +
  // category chips) at the previewed strength — one creature's whole animation set → ONE patch.
  async function handleExportMotionPatchBatch() {
    if (visibleMotions.length === 0 || skeletonNodes.length === 0 || !styleActive) return;
    setExportingPatch(true);
    setPatchMessage(null);
    try {
      const { patch, failed } = await exportMotionPatchBatch(
        visibleMotions[0].archivePath,
        visibleMotions.map((m) => m.entryPath),
        skeletonNodes.map((n) => n.name),
        { smooth: smoothStrength, expressiveness, secondary: secondaryMotion, sharpness },
      );
      setPatchMessage(
        (lang === "uk" ? `Патч зібрано (${visibleMotions.length - failed.length} кліпів` : `Patch built (${visibleMotions.length - failed.length} clips`) +
          (failed.length ? (lang === "uk" ? `, ${failed.length} пропущено` : `, ${failed.length} skipped`) : "") +
          "): " +
          patch +
          (lang === "uk" ? " — встанови в гру в Налаштуваннях (🎮)" : " — install via Settings (🎮)"),
      );
    } catch (e) {
      setPatchMessage(String(e));
    } finally {
      setExportingPatch(false);
    }
  }

  async function handleExportMotionPatch() {
    if (!selectedMotion || skeletonNodes.length === 0 || !styleActive) return;
    setExportingPatch(true);
    setPatchMessage(null);
    try {
      const patch = await exportMotionPatch(
        selectedMotion.archivePath,
        selectedMotion.entryPath,
        skeletonNodes.map((n) => n.name),
        { smooth: smoothStrength, expressiveness, secondary: secondaryMotion, sharpness },
      );
      setPatchMessage(
        (lang === "uk" ? "Патч зібрано: " : "Patch built: ") +
          patch +
          (lang === "uk" ? " — встанови його в гру в Налаштуваннях (🎮)" : " — install it via Settings (🎮)"),
      );
    } catch (e) {
      setPatchMessage(String(e));
    } finally {
      setExportingPatch(false);
    }
  }

  // The real, exportable 60fps (xmot::rebuild_motion_file) — genuinely resizes the .xmot on
  // disk, unlike the doubleRate preview toggle above. UNVERIFIED IN-GAME (see the Rust doc
  // comment) — separate button, separate honest label, so it's never confused with the
  // already-proven smooth/expressiveness/secondary/sharpness export path.
  async function handleExportDoubleRatePatch() {
    if (!selectedMotion || skeletonNodes.length === 0) return;
    setExportingPatch(true);
    setPatchMessage(null);
    try {
      const patch = await exportDoubleRateMotionPatch(selectedMotion.archivePath, selectedMotion.entryPath, skeletonNodes.map((n) => n.name));
      setPatchMessage(
        (lang === "uk" ? "⚠ Експериментальний 60fps-патч зібрано: " : "⚠ Experimental 60fps patch built: ") +
          patch +
          (lang === "uk" ? " — не перевірено в грі, встанови на свій ризик" : " — unverified in-game, install at your own risk"),
      );
    } catch (e) {
      setPatchMessage(String(e));
    } finally {
      setExportingPatch(false);
    }
  }
  const [playing, setPlaying] = useState(true);
  const [showSkeleton, setShowSkeleton] = useState(true);
  const [mirrorSkeleton, setMirrorSkeleton] = useState(false);
  const [mirrorMesh, setMirrorMesh] = useState(false);

  useEffect(() => {
    listActors()
      .then(setActors)
      .catch((e) => setError(String(e)));
    // Real archives: "animations.pak" (2333 real body/gesture clips) vs 4x "speech_<lang>.pak"
    // (~75k real per-line lip-sync/facial clips, one set per localization — see
    // risenlab-animation-research memory). Speech clips are keyed by dialogue line, not by
    // creature, and a creature's own name routinely appears inside an unrelated NPC's spoken
    // line (confirmed on real data: "Ogre" matched 461 speech clips from NPCs like "Don"/
    // "Harbour" discussing ogres, none of them an actual Ogre body animation). This tab is
    // body-skeleton playback, so only the real body/gesture archive belongs here.
    listMotions()
      .then((all) => setMotions(all.filter((m) => m.archiveStem === "animations")))
      .catch(() => {});
    listLibrary()
      .then(setTextures)
      .catch(() => {});
  }, []);

  // Same auto-match as Models.tsx (see there): the actor's own material already references
  // real texture file names, so use those instead of a manual picker.
  useEffect(() => {
    if (!selectedActor || textures.length === 0) {
      setDiffuseUrl(null);
      setNormalUrl(null);
      return;
    }
    let cancelled = false;
    actorTextureRefs(selectedActor.archivePath, selectedActor.entryPath)
      .then(async (refs) => {
        if (cancelled) return;
        const diffuseEntry = refs.diffuse ? findTextureByBaseName(textures, refs.diffuse) : null;
        const normalEntry = refs.normal ? findTextureByBaseName(textures, refs.normal) : null;
        const [dUrl, nUrl] = await Promise.all([
          diffuseEntry ? readTextureDataUrl(diffuseEntry.pngRel) : Promise.resolve(null),
          normalEntry ? readTextureDataUrl(normalEntry.pngRel) : Promise.resolve(null),
        ]);
        if (!cancelled) {
          setDiffuseUrl(dUrl);
          setNormalUrl(nUrl);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setDiffuseUrl(null);
          setNormalUrl(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedActor, textures]);

  const actorTree = useMemo(() => buildFolderTree(actors), [actors]);
  const visibleActors = useMemo(
    () => filterEntries(filterByTreeKey(actors, actorTreeKey), actorQuery),
    [actors, actorTreeKey, actorQuery],
  );
  const visibleMotions = useMemo(() => {
    const byQuery = filterEntries(motions, motionQuery);
    return motionCategoryFilter === "all" ? byQuery : byQuery.filter((m) => motionCategory(m.name) === motionCategoryFilter);
  }, [motions, motionQuery, motionCategoryFilter]);

  // Every real "sit/stand + do something" scenario, derived mechanically from the Hero's own
  // real Interacts clip names (owner, 2026-07-21: "виведи всі сценарії в те меню" — all of them,
  // not just the flute example this started from) — see lib/scenarios.ts.
  const scenarios = useMemo(() => deriveScenarios(motions), [motions]);

  // Selecting a scenario needs the Hero's own skeleton (every real clip a scenario plays is a
  // `Hero_*` clip) — auto-selects `Ani_Hero_Armor_Player` if some other character is currently
  // selected, same convenience `guessMotionQuery` gives regular clip browsing.
  function handleSelectScenario(scenario: ScenarioDef) {
    setSelectedMotion(null);
    setSelectedScenario(scenario);
    if (selectedActor?.name !== "Ani_Hero_Armor_Player._xmac") {
      const hero = actors.find((a) => a.name === "Ani_Hero_Armor_Player._xmac");
      if (hero) setSelectedActor(hero);
    }
  }
  function handleSelectMotion(motion: MotionEntry) {
    setSelectedScenario(null);
    setSelectedMotion(motion);
  }

  // Fetches real bone tracks for every clip in the selected scenario, in order — the SAME
  // `motionTracks` call regular single-clip playback already uses, just once per real clip name
  // instead of once for whichever clip the user clicked. Clip names are looked up in the
  // already-loaded `motions` list (a plain exact-name match, not a new API) so this never
  // depends on knowing an archive path up front.
  useEffect(() => {
    if (!selectedScenario || skeletonNodes.length === 0) {
      setScenarioSteps(null);
      setScenarioError(null);
      return;
    }
    let cancelled = false;
    setScenarioSteps(null);
    setScenarioError(null);
    const boneNames = skeletonNodes.map((n) => n.name);
    (async () => {
      const steps: ScenarioStepTracks[] = [];
      for (const clip of selectedScenario.clips) {
        const entry = motions.find((m) => m.name === clip.name);
        if (!entry) {
          if (!cancelled) setScenarioError(`Кліп не знайдено серед реальних анімацій: ${clip.name}`);
          return;
        }
        const tracks = await motionTracks(entry.archivePath, entry.entryPath, boneNames);
        if (cancelled) return;
        steps.push({ label: clip.label, tracks, sustain: clip.sustain, advanceLabel: clip.advanceLabel });
      }
      if (!cancelled) setScenarioSteps(steps);
    })().catch((e) => {
      if (!cancelled) setScenarioError(String(e));
    });
    return () => {
      cancelled = true;
    };
  }, [selectedScenario, skeletonNodes, motions]);

  // Scenarios are picked from the SAME list as regular clips (owner request: switch between
  // them like before) — pseudo-entries prepended, distinguished from a real `MotionEntry` by
  // carrying a `scenario` field no real entry has.
  const motionListItems = useMemo(
    () => [...scenarios.map((s) => ({ name: s.label, scenario: s })), ...visibleMotions],
    [visibleMotions, scenarios],
  );
  function handleSelectListItem(item: (typeof motionListItems)[number]) {
    if ("scenario" in item) handleSelectScenario(item.scenario);
    else handleSelectMotion(item);
  }
  const selectedListName = selectedScenario ? selectedScenario.label : (selectedMotion?.name ?? null);

  // Per-material texture resolution for multi-material actors (see the matching prop doc in
  // SkeletonAnimationViewer) — same library lookup the auto-match above uses. When the user
  // enhanced this actor's textures and toggled the preview, serve the `edited/` variant for
  // exactly the textures that really have one (see `enhancedRels` above).
  const resolveTexture = useCallback(
    async (baseName: string) => {
      const entry = findTextureEntryForBaseName(textures, baseName);
      if (!entry) return null;
      if (showEnhanced && enhancedRels.has(entry.pngRel)) return readEditedDataUrl(entry.pngRel);
      return readTextureDataUrl(entry.pngRel);
    },
    [textures, showEnhanced, enhancedRels],
  );

  // The two fixed resolvers behind "⿻ Текстури поруч" (owner request, 2026-07-19: a real
  // 2-object original/enhanced TEXTURE comparison for animated actors, distinct from
  // `resolveTexture` above which follows the single `showEnhanced` toggle) — one side always
  // shows the untouched library texture, the other always shows the `edited/` variant where one
  // exists (falls back to original for any material that wasn't part of this enhance batch).
  const resolveTextureOriginal = useCallback(
    async (baseName: string) => {
      const entry = findTextureEntryForBaseName(textures, baseName);
      return entry ? readTextureDataUrl(entry.pngRel) : null;
    },
    [textures],
  );
  const resolveTextureEnhanced = useCallback(
    async (baseName: string) => {
      const entry = findTextureEntryForBaseName(textures, baseName);
      if (!entry) return null;
      return enhancedRels.has(entry.pngRel) ? readEditedDataUrl(entry.pngRel) : readTextureDataUrl(entry.pngRel);
    },
    [textures, enhancedRels],
  );

  // Every distinct diffuse texture the selected actor's own materials reference — the real
  // list "Покращити текстури" works through (e.g. SwampMummy = Eyes + Body + Head diffuse).
  const actorDiffuseEntries = useMemo(() => {
    const mats = skinnedMeshData?.materials ?? [];
    const seen = new Set<string>();
    const entries: LibraryEntry[] = [];
    for (const m of mats) {
      if (!m.diffuse) continue;
      const entry = findTextureEntryForBaseName(textures, m.diffuse);
      if (entry && !seen.has(entry.pngRel)) {
        seen.add(entry.pngRel);
        entries.push(entry);
      }
    }
    return entries;
  }, [skinnedMeshData, textures]);

  async function handleEnhanceTextures() {
    if (actorDiffuseEntries.length === 0) return;
    setEnhancing(true);
    setError(null);
    try {
      const done = new Set(enhancedRels);
      // Sequential on purpose: each regenerate call shells out to the CLI on the dev-server
      // side; parallel calls on a 2-4 texture actor save little and interleave error states.
      for (const entry of actorDiffuseEntries) {
        await regenerateTexture(entry.pngRel);
        done.add(entry.pngRel);
      }
      setEnhancedRels(done);
      setShowEnhanced(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setEnhancing(false);
    }
  }

  useEffect(() => {
    if (!selectedActor) {
      setObjUrl(null);
      return;
    }
    let cancelled = false;
    setObjUrl(null);
    setObjError(null);
    setObjLoading(true);
    setSelectedMotion(null);
    setMotionTracksData(null);
    // Enhancement preview is per-actor: a new selection starts back at the originals (the
    // edited/ files themselves persist on disk — only the toggle resets).
    setShowEnhanced(false);
    setEnhancedRels(new Set());
    // Per-actor, not "one method for everyone" — see lib/actorOrientation.ts for why this
    // isn't a from-scratch geometric guess (one was tried and failed validation).
    const orientation = getActorOrientation(selectedActor.archivePath, selectedActor.entryPath);
    setMirrorSkeleton(orientation.mirrorSkeleton);
    setMirrorMesh(orientation.mirrorMesh);
    setMotionQuery(guessMotionQuery(selectedActor.name));
    actorObjUrl(selectedActor.archivePath, selectedActor.entryPath)
      .then((url) => {
        if (!cancelled) setObjUrl(url);
      })
      .catch((e) => {
        if (!cancelled) setObjError(String(e));
      })
      .finally(() => {
        if (!cancelled) setObjLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedActor]);

  // Real bone hierarchy for the selected actor — needed both to build the skeleton and as
  // the bone-name list to ask the selected motion clip for tracks (see next effect).
  useEffect(() => {
    if (!selectedActor) {
      setSkeletonNodes([]);
      return;
    }
    let cancelled = false;
    setSkeletonError(null);
    actorSkeleton(selectedActor.archivePath, selectedActor.entryPath)
      .then((nodes) => {
        if (!cancelled) setSkeletonNodes(nodes);
      })
      .catch((e) => {
        // Logged with the actor name so a real failure (e.g. a big-endian or otherwise
        // unsupported actor file) is diagnosable from the console instead of just a blank
        // viewport with no visible cause.
        console.error(`[Animations] actorSkeleton failed for ${selectedActor.name}:`, e);
        if (!cancelled) setSkeletonError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [selectedActor]);

  // Real per-vertex bone weights, so the mesh surface actually deforms with the skeleton
  // instead of staying a static bind pose. Best-effort: falls back to the static .obj (via
  // `objUrl`, already loaded elsewhere) if this specific actor's skin data can't be parsed.
  useEffect(() => {
    if (!selectedActor) {
      setSkinnedMeshData(null);
      return;
    }
    let cancelled = false;
    setSkinnedMeshData(null);
    actorSkinnedMesh(selectedActor.archivePath, selectedActor.entryPath)
      .then((mesh) => {
        if (cancelled) return;
        setSkinnedMeshData(mesh);
        // Real skin data is fetched and kept in state (see the render below, currently not
        // wired into SkeletonAnimationViewer — see the comment there) so a follow-up session
        // can pick this up directly from devtools without re-fetching.
        console.log(`[Animations] real skin data for ${selectedActor.name}: ${mesh.positions.length} verts, ${mesh.skinWeights.filter((w) => w.length > 0).length} skinned`);
      })
      .catch((e) => {
        console.error(`[Animations] actorSkinnedMesh failed for ${selectedActor.name}, falling back to static mesh:`, e);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedActor]);

  useEffect(() => {
    if (!selectedMotion || skeletonNodes.length === 0) {
      setMotionTracksData(null);
      return;
    }
    let cancelled = false;
    setTracksLoading(true);
    setPlaying(true);
    setAbOriginal(false);
    const names = skeletonNodes.map((n) => n.name);
    Promise.all([
      motionTracks(selectedMotion.archivePath, selectedMotion.entryPath, names),
      previewActive
        ? motionTracks(selectedMotion.archivePath, selectedMotion.entryPath, names, {
            smooth: smoothStrength,
            expressiveness,
            secondary: secondaryMotion,
            sharpness,
            doubleRate,
          })
        : Promise.resolve(null),
    ])
      .then(([orig, styled]) => {
        if (cancelled) return;
        setOriginalTracks(orig);
        setMotionTracksData(styled ?? orig);
      })
      .catch(() => {
        if (!cancelled) setMotionTracksData(null);
      })
      .finally(() => {
        if (!cancelled) setTracksLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedMotion, skeletonNodes, smoothStrength, expressiveness, secondaryMotion, sharpness, doubleRate, previewActive]);

  return (
    <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
      <FolderTree nodes={actorTree} selectedKey={actorTreeKey} onSelect={setActorTreeKey} title={lang === "uk" ? "Архіви" : "Archives"} />

      <div
        style={{
          width: 236,
          flexShrink: 0,
          background: "var(--bg1)",
          borderRight: "1px solid var(--border)",
          display: "flex",
          flexDirection: "column",
          minHeight: 0,
          padding: "14px 10px",
        }}
      >
        <div
          style={{
            font: "600 10px system-ui",
            letterSpacing: ".06em",
            textTransform: "uppercase",
            color: "var(--text-faint)",
            padding: "0 2px 8px",
          }}
        >
          {lang === "uk" ? `Персонажі (${actors.length} реальних)` : `Characters (${actors.length} real)`}
        </div>
        <SearchableList
          items={visibleActors}
          selectedName={selectedActor?.name ?? null}
          onSelect={setSelectedActor}
          query={actorQuery}
          onQueryChange={setActorQuery}
          placeholder={lang === "uk" ? "Пошук персонажа…" : "Search characters…"}
          limit={150}
        />
      </div>

      <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, minHeight: 0 }}>
        <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
          <div style={{ flex: 1, minHeight: 0, position: "relative" }}>
            {!selectedActor ? (
              <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
                {lang === "uk" ? "Оберіть персонажа зліва" : "Select a character on the left"}
              </div>
            ) : selectedScenario && scenarioError ? (
              <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--red)" }}>{scenarioError}</div>
            ) : selectedScenario && scenarioSteps ? (
              <ScenarioPlayer
                key={selectedScenario.id}
                nodes={skeletonNodes}
                steps={scenarioSteps}
                playing={playing}
                showSkeleton={showSkeleton}
                mirrorSkeleton={mirrorSkeleton}
                mirrorMesh={mirrorMesh}
                skinnedMesh={skinnedMeshData}
                objUrl={objUrl}
                diffuseUrl={diffuseUrl}
                normalUrl={normalUrl}
                resolveTexture={resolveTexture}
              />
            ) : selectedScenario ? (
              <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
                {lang === "uk" ? "Завантаження реальних кадрів сценарію…" : "Loading real scenario keyframes…"}
              </div>
            ) : textureSideBySide && enhancedRels.size > 0 && (motionTracksData ? true : !!objUrl) ? (
              <div style={{ height: "100%", display: "flex", gap: 2 }}>
                {([
                  [lang === "uk" ? "Оригінал" : "Original", resolveTextureOriginal],
                  [lang === "uk" ? "Покращено" : "Enhanced", resolveTextureEnhanced],
                ] as [string, (baseName: string) => Promise<string | null>][]).map(([label, resolver], i) =>
                  selectedMotion && motionTracksData ? (
                    <div key={i} style={{ flex: 1, position: "relative", minWidth: 0, borderLeft: i === 1 ? "1px solid var(--border)" : "none" }}>
                      <SkeletonAnimationViewer
                        key={`${selectedActor.entryPath}::${selectedMotion.entryPath}::texsxs${i}`}
                        nodes={skeletonNodes}
                        tracks={abOriginal && originalTracks ? originalTracks : motionTracksData}
                        playing={playing}
                        showSkeleton={showSkeleton}
                        mirrorSkeleton={mirrorSkeleton}
                        mirrorMesh={mirrorMesh}
                        skinnedMesh={skinnedMeshData}
                        objUrl={objUrl}
                        diffuseUrl={diffuseUrl}
                        normalUrl={normalUrl}
                        resolveTexture={resolver}
                        cameraSync={cameraSyncRef}
                      />
                      <div style={{ position: "absolute", top: 10, left: 10, font: "700 10px system-ui", color: i === 1 ? "#fff" : "var(--text-dim)", background: i === 1 ? "var(--accent)" : "rgba(0,0,0,.45)", padding: "4px 9px", borderRadius: 6, pointerEvents: "none" }}>
                        {label}
                      </div>
                    </div>
                  ) : (
                    <div key={i} style={{ flex: 1, position: "relative", minWidth: 0, borderLeft: i === 1 ? "1px solid var(--border)" : "none" }}>
                      <Model3DViewer
                        key={`${selectedActor.entryPath}::texsxs${i}`}
                        objUrl={objUrl ?? ""}
                        diffuseUrl={diffuseUrl}
                        normalUrl={normalUrl}
                        mode={mode}
                        resolveTexture={resolver}
                        cameraSync={cameraSyncRef}
                      />
                      <div style={{ position: "absolute", top: 10, left: 10, font: "700 10px system-ui", color: i === 1 ? "#fff" : "var(--text-dim)", background: i === 1 ? "var(--accent)" : "rgba(0,0,0,.45)", padding: "4px 9px", borderRadius: 6, pointerEvents: "none" }}>
                        {label}
                      </div>
                    </div>
                  ),
                )}
              </div>
            ) : selectedMotion && motionTracksData && sideBySide && originalTracks && previewActive ? (
              <div style={{ height: "100%", display: "flex", gap: 2 }}>
                {([
                  [lang === "uk" ? "Оригінал" : "Original", originalTracks, "var(--text-faint)"],
                  [lang === "uk" ? "Стилізована" : "Styled", motionTracksData, "var(--accent)"],
                ] as [string, BoneMotion[], string][]).map(([label, tracks, color], i) => (
                  <div key={i} style={{ flex: 1, position: "relative", minWidth: 0, borderLeft: i === 1 ? "1px solid var(--border)" : "none" }}>
                    <SkeletonAnimationViewer
                      key={`${selectedActor.entryPath}::${selectedMotion.entryPath}::sxs${i}`}
                      nodes={skeletonNodes}
                      tracks={tracks}
                      playing={playing}
                      showSkeleton={showSkeleton}
                      mirrorSkeleton={mirrorSkeleton}
                      mirrorMesh={mirrorMesh}
                      skinnedMesh={skinnedMeshData}
                      objUrl={objUrl}
                      diffuseUrl={diffuseUrl}
                      normalUrl={normalUrl}
                      resolveTexture={resolveTexture}
                      cameraSync={cameraSyncRef}
                    />
                    <div style={{ position: "absolute", top: 10, left: 10, font: "700 10px system-ui", color: i === 1 ? "#fff" : "var(--text-dim)", background: i === 1 ? "var(--accent)" : "rgba(0,0,0,.45)", padding: "4px 9px", borderRadius: 6, pointerEvents: "none", borderColor: color }}>
                      {label}
                    </div>
                  </div>
                ))}
              </div>
            ) : selectedMotion && motionTracksData ? (
              <SkeletonAnimationViewer
                key={`${selectedActor.entryPath}::${selectedMotion.entryPath}`}
                nodes={skeletonNodes}
                tracks={abOriginal && originalTracks ? originalTracks : motionTracksData}
                playing={playing}
                showSkeleton={showSkeleton}
                mirrorSkeleton={mirrorSkeleton}
                mirrorMesh={mirrorMesh}
                // Real per-vertex bone weights, rendered via CPU skinning in
                // SkeletonAnimationViewer (see there for why: THREE.SkinnedMesh's built-in GPU
                // path had an unresolved rendering bug). Falls back to the static .obj (`objUrl`)
                // if this is null (e.g. an actor whose skin data failed to parse).
                skinnedMesh={skinnedMeshData}
                objUrl={objUrl}
                diffuseUrl={diffuseUrl}
                normalUrl={normalUrl}
                resolveTexture={resolveTexture}
              />
            ) : skeletonError ? (
              <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--red)" }}>
                {skeletonError}
              </div>
            ) : objError ? (
              <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--red)" }}>
                {objError}
              </div>
            ) : objLoading || !objUrl ? (
              <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
                {lang === "uk" ? "Конвертація актора…" : "Converting actor…"}
              </div>
            ) : (
              <Model3DViewer
                key={selectedActor.entryPath}
                objUrl={objUrl}
                diffuseUrl={diffuseUrl}
                normalUrl={normalUrl}
                mode={mode}
                resolveTexture={resolveTexture}
              />
            )}
            {tracksLoading ? (
              <div style={{ position: "absolute", top: 10, left: 10, font: "600 11px system-ui", color: "var(--text-faint)", background: "rgba(0,0,0,.45)", padding: "4px 9px", borderRadius: 6, pointerEvents: "none" }}>
                {lang === "uk" ? "Завантаження реальних кадрів анімації…" : "Loading real keyframe data…"}
              </div>
            ) : null}
          </div>

          {/* Right control sidebar (owner request, 2026-07-18): everything that used to sit in
              a crowded row above the 3D canvas now lives here, next to it, on a wide screen. */}
          <div style={{ width: 268, flexShrink: 0, borderLeft: "1px solid var(--border)", background: "var(--bg1)", overflowY: "auto", padding: "12px 14px" }}>
            <div style={{ display: "flex", gap: 4 }}>
              {(["textured", "wireframe", "clay", "normalMap"] as ViewMode[]).map((m) => (
                <button
                  key={m}
                  onClick={() => setMode(m)}
                  style={{
                    flex: 1,
                    padding: "6px 4px",
                    borderRadius: 10,
                    background: mode === m ? "var(--accent)" : "var(--bg2)",
                    border: `1px solid ${mode === m ? "var(--accent)" : "var(--border)"}`,
                    font: "600 10px system-ui",
                    color: mode === m ? "#fff" : "var(--text-dim)",
                  }}
                >
                  {m}
                </button>
              ))}
            </div>

            {selectedActor && actorDiffuseEntries.length > 0 ? (
              <>
                <div style={sidebarDivider} />
                <button
                  onClick={handleEnhanceTextures}
                  disabled={enhancing}
                  style={{
                    width: "100%",
                    padding: "7px 10px",
                    borderRadius: 10,
                    background: enhancing ? "var(--bg2)" : "var(--accent)",
                    border: "1px solid var(--accent)",
                    font: "600 11px system-ui",
                    color: enhancing ? "var(--text-dim)" : "#fff",
                    cursor: enhancing ? "wait" : "pointer",
                  }}
                  title={
                    lang === "uk"
                      ? `Покращити всі текстури цієї моделі (${actorDiffuseEntries.length} шт.)`
                      : `Enhance all of this model's textures (${actorDiffuseEntries.length})`
                  }
                >
                  {enhancing
                    ? lang === "uk" ? "Покращення…" : "Enhancing…"
                    : lang === "uk" ? `✨ Покращити текстури (${actorDiffuseEntries.length})` : `✨ Enhance textures (${actorDiffuseEntries.length})`}
                </button>
                {enhancedRels.size > 0 ? (
                  <>
                    <button
                      onClick={() => setShowEnhanced((v) => !v)}
                      style={{
                        width: "100%",
                        marginTop: 6,
                        padding: "6px 10px",
                        borderRadius: 10,
                        background: showEnhanced ? "var(--accent)" : "var(--bg2)",
                        border: `1px solid ${showEnhanced ? "var(--accent)" : "var(--border)"}`,
                        font: "600 10.5px system-ui",
                        color: showEnhanced ? "#fff" : "var(--text-dim)",
                      }}
                    >
                      {showEnhanced
                        ? lang === "uk" ? "Показано: покращені" : "Showing: enhanced"
                        : lang === "uk" ? "Показано: оригінал" : "Showing: original"}
                    </button>
                    <button
                      onClick={() => {
                        setTextureSideBySide((v) => !v);
                        setSideBySide(false);
                      }}
                      title={
                        lang === "uk"
                          ? "Два об'єкти одночасно: зліва оригінальні текстури, справа покращені — та сама поза/анімація."
                          : "Two objects at once: original textures left, enhanced right — same pose/animation."
                      }
                      style={{
                        width: "100%",
                        marginTop: 6,
                        padding: "6px 10px",
                        borderRadius: 10,
                        background: textureSideBySide ? "var(--accent)" : "var(--bg2)",
                        border: `1px solid ${textureSideBySide ? "var(--accent)" : "var(--border)"}`,
                        font: "600 10.5px system-ui",
                        color: textureSideBySide ? "#fff" : "var(--text-dim)",
                      }}
                    >
                      {lang === "uk" ? "⿻ Текстури поруч" : "⿻ Textures side by side"}
                    </button>
                  </>
                ) : null}
              </>
            ) : null}

            {selectedMotion && motionTracksData ? (
              <>
                <div style={sidebarDivider} />
                <div style={{ font: "500 10.5px system-ui", color: "var(--text-dim)", lineHeight: 1.4, marginBottom: 10 }}>
                  {lang === "uk"
                    ? `${motionDuration(motionTracksData).toFixed(2)}с, ${
                        motionTracksData.filter((t) => t.positionKeys.length > 0 || t.rotationKeys.length > 0).length
                      } кісток анімовано`
                    : `${motionDuration(motionTracksData).toFixed(2)}s, ${
                        motionTracksData.filter((t) => t.positionKeys.length > 0 || t.rotationKeys.length > 0).length
                      } bones animated`}
                </div>
                <button
                  onClick={() => setPlaying((p) => !p)}
                  style={{
                    width: "100%",
                    padding: "7px 10px",
                    borderRadius: 10,
                    background: "var(--accent)",
                    border: "1px solid var(--accent)",
                    font: "600 11px system-ui",
                    color: "#fff",
                    marginBottom: 8,
                  }}
                >
                  {playing ? (lang === "uk" ? "⏸ Пауза" : "⏸ Pause") : (lang === "uk" ? "▶ Грати" : "▶ Play")}
                </button>
                <label style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer", font: "500 11px system-ui", marginBottom: 4 }}>
                  <input type="checkbox" checked={showSkeleton} onChange={(e) => setShowSkeleton(e.target.checked)} />
                  {lang === "uk" ? "Показати кістки" : "Show skeleton"}
                </label>
                <label
                  style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer", font: "500 11px system-ui", marginBottom: 4 }}
                  title={lang === "uk" ? "Запам'ятовується окремо для цього персонажа" : "Remembered per character"}
                >
                  <input
                    type="checkbox"
                    checked={mirrorSkeleton}
                    onChange={(e) => {
                      const next = e.target.checked;
                      setMirrorSkeleton(next);
                      if (selectedActor) setActorOrientation(selectedActor.archivePath, selectedActor.entryPath, { mirrorSkeleton: next, mirrorMesh });
                    }}
                  />
                  {lang === "uk" ? "Перевернути кістки" : "Flip skeleton"}
                </label>
                <label
                  style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer", font: "500 11px system-ui" }}
                  title={lang === "uk" ? "Запам'ятовується окремо для цього персонажа" : "Remembered per character"}
                >
                  <input
                    type="checkbox"
                    checked={mirrorMesh}
                    onChange={(e) => {
                      const next = e.target.checked;
                      setMirrorMesh(next);
                      if (selectedActor) setActorOrientation(selectedActor.archivePath, selectedActor.entryPath, { mirrorSkeleton, mirrorMesh: next });
                    }}
                  />
                  {lang === "uk" ? "Перевернути сітку" : "Flip mesh"}
                </label>

                <div style={sidebarSectionTitle}>{lang === "uk" ? "Якість анімації" : "Animation quality"}</div>
                <QualityToggle
                  label={lang === "uk" ? "🎬 Дрижання" : "🎬 Jitter"}
                  title={
                    lang === "uk"
                      ? "Реальний фільтр очищення дрижання суглобів (rust: smooth_tracks) — те саме, що запишеться у .xmot при експорті."
                      : "Real joint-jitter cleanup filter (rust: smooth_tracks) — exactly what a .xmot export would contain."
                  }
                  value={smoothStrength}
                  onChange={setSmoothStrength}
                  disabled={tracksLoading}
                  options={[
                    [0, lang === "uk" ? "ориг." : "orig"],
                    [0.35, lang === "uk" ? "м'яко" : "soft"],
                    [0.6, lang === "uk" ? "сильно" : "strong"],
                  ]}
                />
                <QualityToggle
                  label={lang === "uk" ? "💪 Виразність" : "💪 Expressiveness"}
                  title={
                    lang === "uk"
                      ? "Підсилює амплітуду руху корпуса/рук/голови відносно середньої пози. Ноги й корінь не чіпає — щоб не ковзали по землі."
                      : "Amplifies torso/arm/head motion away from the average pose. Legs and root are left untouched so the character doesn't slide."
                  }
                  value={expressiveness}
                  onChange={setExpressiveness}
                  disabled={tracksLoading}
                  options={[
                    [0, lang === "uk" ? "ориг." : "orig"],
                    [0.25, lang === "uk" ? "помітно" : "visible"],
                    [0.4, lang === "uk" ? "сильно" : "strong"],
                  ]}
                />
                <QualityToggle
                  label={lang === "uk" ? "🌊 Вторинний рух" : "🌊 Secondary motion"}
                  title={
                    lang === "uk"
                      ? "Хвости/тканина/вуха/волосся/пасок отримують запізнення (follow-through) і більшу амплітуду відносно тіла."
                      : "Tails/cloth/ears/hair/belt get a delayed follow-through and extra amplitude relative to the body."
                  }
                  value={secondaryMotion}
                  onChange={setSecondaryMotion}
                  disabled={tracksLoading}
                  options={[
                    [0, lang === "uk" ? "ориг." : "orig"],
                    [0.35, lang === "uk" ? "помітно" : "visible"],
                    [0.6, lang === "uk" ? "сильно" : "strong"],
                  ]}
                />
                <QualityToggle
                  label={lang === "uk" ? "⚡ Різкість ударів" : "⚡ Strike sharpness"}
                  title={
                    lang === "uk"
                      ? "Перетаймінг: повільний замах, різкий удар — часи ключів переписуються, значення не змінюються. Найкорисніше на бойових кліпах."
                      : "Retiming: slow windup, sharp strike — key TIMES are rewritten, values untouched. Most useful on combat clips."
                  }
                  value={sharpness}
                  onChange={setSharpness}
                  disabled={tracksLoading}
                  options={[
                    [0, lang === "uk" ? "ориг." : "orig"],
                    [0.5, lang === "uk" ? "помітно" : "visible"],
                    [0.8, lang === "uk" ? "сильно" : "strong"],
                  ]}
                />
                <label
                  style={{ display: "flex", alignItems: "center", gap: 6, cursor: tracksLoading ? "wait" : "pointer", font: "500 11px system-ui", marginTop: 4 }}
                  title={
                    lang === "uk"
                      ? "Подвоює частоту кадрів для плавнішого перегляду ТУТ (лінійна/slerp інтерполяція проміжних ключів). ⚠️ Тільки перегляд — у патч це поки не запишеться: формат потребує зміни кількості ключів, а обгортку .xmot ще не розшифровано до кінця."
                      : "Doubles the frame rate for smoother playback HERE (linear/slerp interpolated in-between keys). ⚠️ Preview only — can't be written into a patch yet: that needs changing key counts, and the .xmot wrapper isn't fully decoded."
                  }
                >
                  <input type="checkbox" checked={doubleRate} disabled={tracksLoading} onChange={(e) => setDoubleRate(e.target.checked)} />
                  {lang === "uk" ? "🎬 60fps (тільки перегляд)" : "🎬 60fps (preview only)"}
                </label>
                {doubleRate ? (
                  <button
                    onClick={handleExportDoubleRatePatch}
                    disabled={exportingPatch}
                    title={
                      lang === "uk"
                        ? "Реальний патч зі зміненою кількістю ключів (а не просто перегляд) — формат щойно розшифрований, НЕ перевірено в грі."
                        : "A real patch with a genuinely changed key count (not just preview) — the format was just reverse-engineered, NOT verified in-game."
                    }
                    style={{
                      width: "100%",
                      marginTop: 6,
                      padding: "6px 10px",
                      borderRadius: 10,
                      background: "var(--bg2)",
                      border: "1px solid var(--red)",
                      font: "600 10.5px system-ui",
                      color: exportingPatch ? "var(--text-faint)" : "var(--red)",
                      cursor: exportingPatch ? "wait" : "pointer",
                    }}
                  >
                    {lang === "uk" ? "⚠ 💾 Реальний 60fps-патч (експер.)" : "⚠ 💾 Real 60fps patch (exp.)"}
                  </button>
                ) : null}

                {previewActive ? (
                  <>
                    <div style={sidebarDivider} />
                    {styleActive ? (
                      <>
                        <button
                          onClick={handleExportMotionPatch}
                          disabled={exportingPatch}
                          title={
                            lang === "uk"
                              ? "Зібрати патч-том animations.pNN з цим кліпом у поточному стилі — потім «Встановити в гру» в Налаштуваннях."
                              : "Build an animations.pNN patch volume with this clip at the current style — then “Install into game” in Settings."
                          }
                          style={{
                            width: "100%",
                            padding: "7px 10px",
                            borderRadius: 10,
                            background: exportingPatch ? "var(--bg2)" : "var(--accent)",
                            border: "1px solid var(--accent)",
                            font: "600 11px system-ui",
                            color: exportingPatch ? "var(--text-dim)" : "#fff",
                            cursor: exportingPatch ? "wait" : "pointer",
                            marginBottom: 6,
                          }}
                        >
                          {exportingPatch ? (lang === "uk" ? "Збирання…" : "Building…") : lang === "uk" ? "💾 У патч" : "💾 To patch"}
                        </button>
                        {visibleMotions.length > 1 ? (
                          <button
                            onClick={handleExportMotionPatchBatch}
                            disabled={exportingPatch}
                            title={
                              lang === "uk"
                                ? "Стилізувати ВСІ кліпи з поточного списку знизу (фільтр істоти + пошук + категорія) і зібрати їх ОДНИМ патч-томом."
                                : "Style EVERY clip in the current list below (creature filter + search + category) and pack them as ONE patch volume."
                            }
                            style={{
                              width: "100%",
                              padding: "6px 10px",
                              borderRadius: 10,
                              background: "var(--bg2)",
                              border: "1px solid var(--accent)",
                              font: "600 10.5px system-ui",
                              color: exportingPatch ? "var(--text-faint)" : "var(--text)",
                              cursor: exportingPatch ? "wait" : "pointer",
                              marginBottom: 6,
                            }}
                          >
                            {lang === "uk" ? `💾 Всі кліпи (${visibleMotions.length})` : `💾 All clips (${visibleMotions.length})`}
                          </button>
                        ) : null}
                      </>
                    ) : (
                      <div style={{ font: "500 10.5px system-ui", color: "var(--text-faint)", marginBottom: 6 }}>
                        {lang === "uk"
                          ? "60fps — лише перегляд, у патч поки не пишеться."
                          : "60fps is preview-only, not exportable to a patch yet."}
                      </div>
                    )}
                    <div style={{ display: "flex", gap: 6 }}>
                      <button
                        onClick={() => setAbOriginal((v) => !v)}
                        title={
                          lang === "uk"
                            ? "Миттєве перемикання між оригінальним кліпом і стилізованим — той самий момент часу, ті самі кістки."
                            : "Instant flip between the original clip and the styled one — same viewer, only keyframes differ."
                        }
                        style={{
                          flex: 1,
                          padding: "6px 4px",
                          borderRadius: 10,
                          background: abOriginal ? "var(--red)" : "var(--bg2)",
                          border: `1px solid ${abOriginal ? "var(--red)" : "var(--border)"}`,
                          font: "600 10.5px system-ui",
                          color: abOriginal ? "#fff" : "var(--text-dim)",
                        }}
                      >
                        {abOriginal ? (lang === "uk" ? "👁 Оригінал" : "👁 Original") : "A/B"}
                      </button>
                      <button
                        onClick={() => {
                          setSideBySide((v) => !v);
                          setTextureSideBySide(false);
                        }}
                        title={
                          lang === "uk"
                            ? "Дві анімації одночасно: зліва оригінал, справа стилізована (спільні пауза/грати)."
                            : "Both animations at once: original left, styled right (shared play/pause)."
                        }
                        style={{
                          flex: 1,
                          padding: "6px 4px",
                          borderRadius: 10,
                          background: sideBySide ? "var(--accent)" : "var(--bg2)",
                          border: `1px solid ${sideBySide ? "var(--accent)" : "var(--border)"}`,
                          font: "600 10.5px system-ui",
                          color: sideBySide ? "#fff" : "var(--text-dim)",
                        }}
                      >
                        {lang === "uk" ? "⿻ Поруч" : "⿻ Side by side"}
                      </button>
                    </div>
                  </>
                ) : null}
              </>
            ) : (
              <div style={{ font: "500 11px system-ui", color: "var(--text-faint)", marginTop: 12 }}>
                {lang === "uk" ? "Оберіть анімацію знизу, щоб побачити реальне відтворення кістяка." : "Select an animation below to see real skeleton playback."}
              </div>
            )}
          </div>
        </div>

        <div style={{ flexShrink: 0, height: 220, borderTop: "1px solid var(--border)", display: "flex", flexDirection: "column", padding: "10px 16px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 6 }}>
            <div style={{ font: "600 10px system-ui", letterSpacing: ".06em", textTransform: "uppercase", color: "var(--text-faint)" }}>
              {lang === "uk"
                ? `Анімації (${visibleMotions.length} з ${motions.length} реальних)`
                : `Animations (${visibleMotions.length} of ${motions.length} real)`}
            </div>
            <div style={{ display: "flex", gap: 4 }}>
              {MOTION_CATEGORIES.map((c) => (
                <button
                  key={c.id}
                  onClick={() => setMotionCategoryFilter(c.id)}
                  style={{
                    padding: "3px 9px",
                    borderRadius: 10,
                    background: motionCategoryFilter === c.id ? "var(--accent)" : "var(--bg2)",
                    border: `1px solid ${motionCategoryFilter === c.id ? "var(--accent)" : "var(--border)"}`,
                    font: "600 10.5px system-ui",
                    color: motionCategoryFilter === c.id ? "#fff" : "var(--text-dim)",
                    whiteSpace: "nowrap",
                  }}
                >
                  {lang === "uk" ? c.uk : c.en}
                </button>
              ))}
            </div>
          </div>
          <div style={{ display: "flex", gap: 6, overflow: "auto", flex: 1 }}>
            <SearchableList
              items={motionListItems}
              selectedName={selectedListName}
              onSelect={handleSelectListItem}
              query={motionQuery}
              onQueryChange={setMotionQuery}
              placeholder={lang === "uk" ? "Пошук анімації або сценарію…" : "Search animations or scenarios…"}
              limit={150}
            />
          </div>
        </div>
      </div>

      {error ? (
        <div style={{ position: "absolute", bottom: 12, right: 12, color: "var(--red)", font: "500 12px system-ui" }}>{error}</div>
      ) : patchMessage ? (
        <div
          style={{
            position: "absolute",
            bottom: 12,
            right: 12,
            maxWidth: 560,
            background: "var(--bg1)",
            border: "1px solid var(--border)",
            borderRadius: 10,
            padding: "8px 14px",
            color: "var(--text)",
            font: "500 12px system-ui",
            wordBreak: "break-all",
          }}
        >
          {patchMessage}
        </div>
      ) : null}
    </div>
  );
}
