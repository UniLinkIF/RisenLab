import { useEffect, useMemo, useState } from "react";
import type { Lang } from "../lib/i18n";
import type { ActorEntry, BoneMotion, LibraryEntry, MotionEntry, SkeletonNode, SkinnedMeshData } from "../lib/types";
import { actorObjUrl, actorSkeleton, actorSkinnedMesh, actorTextureRefs, listActors, listLibrary, listMotions, motionTracks, readTextureDataUrl } from "../lib/api";
import { buildFolderTree, filterByTreeKey, filterEntries, findTextureByBaseName } from "../lib/library";
import { getActorOrientation, setActorOrientation } from "../lib/actorOrientation";
import FolderTree from "../components/FolderTree";
import Model3DViewer, { type ViewMode } from "../components/Model3DViewer";
import SkeletonAnimationViewer, { motionDuration } from "../components/SkeletonAnimationViewer";
import SearchableList from "../components/SearchableList";

interface Props {
  lang: Lang;
}

/** Actor filenames look like "Ani_Wolf_Monster_Wolf._xmac" or "Ani_Hero_Head_Player._xmac";
 * motion clips are named "Wolf_Stand_..." / "Hero_Stand_...". There's no exact ID linking the
 * two real formats (see risenlab-presentation-deadline memory), so this pulls a best-guess
 * character token out of the actor name to pre-filter the motion list — a starting point the
 * user can always broaden by editing the search box themselves, not a guaranteed match.
 *
 * Only real "Ani_"-prefixed character actors get a guess. The other two real prefixes —
 * "It_" (weapon/item props, e.g. "It_Wpn_Crossbow_War") and "Object_" (animated interactables,
 * e.g. "Object_Interact_Animated_Cupboard") — have no dedicated body skeleton animation of
 * their own (they're rigid props driven by the wielder/scene), so their first token is
 * meaningless as a filter and, worse, is often short enough (e.g. "It") to substring-match
 * huge swaths of unrelated real clips (confirmed against real game data: "It" matched 4865
 * motions across Titan/Ogre/Lizard/Goblin). Guessing nothing (empty query = browse everything)
 * beats guessing wrong. */
function guessMotionQuery(actorName: string): string {
  if (!actorName.startsWith("Ani_")) return "";
  const stem = actorName.replace(/\._xmac$/i, "");
  const tokens = stem.split("_").filter((t) => t && t !== "Ani");
  return tokens[0] ?? "";
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
  const [selectedMotion, setSelectedMotion] = useState<MotionEntry | null>(null);

  const [mode, setMode] = useState<ViewMode>("textured");

  const [skeletonNodes, setSkeletonNodes] = useState<SkeletonNode[]>([]);
  const [skeletonError, setSkeletonError] = useState<string | null>(null);
  const [skinnedMeshData, setSkinnedMeshData] = useState<SkinnedMeshData | null>(null);
  const [motionTracksData, setMotionTracksData] = useState<BoneMotion[] | null>(null);
  const [tracksLoading, setTracksLoading] = useState(false);
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
  const visibleMotions = useMemo(() => filterEntries(motions, motionQuery), [motions, motionQuery]);

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
    motionTracks(
      selectedMotion.archivePath,
      selectedMotion.entryPath,
      skeletonNodes.map((n) => n.name),
    )
      .then((tracks) => {
        if (!cancelled) setMotionTracksData(tracks);
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
  }, [selectedMotion, skeletonNodes]);

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
        {selectedMotion && motionTracksData ? (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "12px 20px",
              borderBottom: "1px solid var(--border)",
              font: "500 12px system-ui",
              color: "var(--accent)",
              background: "var(--bg1)",
            }}
          >
            <button
              onClick={() => setPlaying((p) => !p)}
              style={{
                padding: "6px 14px",
                borderRadius: 16,
                background: "var(--accent)",
                border: "1px solid var(--accent)",
                font: "600 11.5px system-ui",
                color: "#fff",
              }}
            >
              {playing ? (lang === "uk" ? "⏸ Пауза" : "⏸ Pause") : (lang === "uk" ? "▶ Грати" : "▶ Play")}
            </button>
            <label style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}>
              <input type="checkbox" checked={showSkeleton} onChange={(e) => setShowSkeleton(e.target.checked)} />
              {lang === "uk" ? "Показати кістки" : "Show skeleton"}
            </label>
            <label
              style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}
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
              style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}
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
            <span>
              {lang === "uk"
                ? `Реальні дані анімації: ${motionDuration(motionTracksData).toFixed(2)}с, ${
                    motionTracksData.filter((t) => t.positionKeys.length > 0 || t.rotationKeys.length > 0).length
                  } кісток анімовано (скелет, без обтягнутої сітки)`
                : `Real keyframe data: ${motionDuration(motionTracksData).toFixed(2)}s, ${
                    motionTracksData.filter((t) => t.positionKeys.length > 0 || t.rotationKeys.length > 0).length
                  } bones animated (skeleton only, not yet skinned to the mesh surface)`}
            </span>
          </div>
        ) : (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "12px 20px",
              borderBottom: "1px solid var(--border)",
              font: "500 12px system-ui",
              color: "var(--accent)",
              background: "var(--bg1)",
            }}
          >
            {tracksLoading
              ? lang === "uk" ? "Завантаження реальних кадрів анімації…" : "Loading real keyframe data…"
              : lang === "uk"
                ? "Оберіть анімацію знизу, щоб побачити реальне відтворення кістяка."
                : "Select an animation below to see real skeleton playback."}
          </div>
        )}
        <div style={{ display: "flex", gap: 8, padding: "12px 20px", borderBottom: "1px solid var(--border)" }}>
          {(["textured", "wireframe", "clay", "normalMap"] as ViewMode[]).map((m) => (
            <button
              key={m}
              onClick={() => setMode(m)}
              style={{
                padding: "7px 14px",
                borderRadius: 16,
                background: mode === m ? "var(--accent)" : "var(--bg2)",
                border: `1px solid ${mode === m ? "var(--accent)" : "var(--border)"}`,
                font: "600 11.5px system-ui",
                color: mode === m ? "#fff" : "var(--text-dim)",
              }}
            >
              {m}
            </button>
          ))}
        </div>
        <div style={{ flex: 1, minHeight: 0, position: "relative" }}>
          {!selectedActor ? (
            <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
              {lang === "uk" ? "Оберіть персонажа зліва" : "Select a character on the left"}
            </div>
          ) : selectedMotion && motionTracksData ? (
            <SkeletonAnimationViewer
              key={`${selectedActor.entryPath}::${selectedMotion.entryPath}`}
              nodes={skeletonNodes}
              tracks={motionTracksData}
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
            <Model3DViewer key={selectedActor.entryPath} objUrl={objUrl} diffuseUrl={diffuseUrl} normalUrl={normalUrl} mode={mode} />
          )}
        </div>

        <div style={{ flexShrink: 0, height: 220, borderTop: "1px solid var(--border)", display: "flex", flexDirection: "column", padding: "10px 16px" }}>
          <div style={{ font: "600 10px system-ui", letterSpacing: ".06em", textTransform: "uppercase", color: "var(--text-faint)", marginBottom: 6 }}>
            {lang === "uk"
              ? `Анімації (${visibleMotions.length} з ${motions.length} реальних)`
              : `Animations (${visibleMotions.length} of ${motions.length} real)`}
          </div>
          <div style={{ display: "flex", gap: 6, overflow: "auto", flex: 1 }}>
            <SearchableList
              items={visibleMotions}
              selectedName={selectedMotion?.name ?? null}
              onSelect={setSelectedMotion}
              query={motionQuery}
              onQueryChange={setMotionQuery}
              placeholder={lang === "uk" ? "Пошук анімації…" : "Search animations…"}
              limit={150}
            />
          </div>
        </div>
      </div>

      {error ? (
        <div style={{ position: "absolute", bottom: 12, right: 12, color: "var(--red)", font: "500 12px system-ui" }}>{error}</div>
      ) : null}
    </div>
  );
}
