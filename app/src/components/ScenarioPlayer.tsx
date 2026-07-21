import { useEffect, useState } from "react";
import type { BoneMotion, SkeletonNode, SkinnedMeshData } from "../lib/types";
import SkeletonAnimationViewer from "./SkeletonAnimationViewer";

export interface ScenarioStepTracks {
  label: string;
  tracks: BoneMotion[];
  /** True = this step loops forever and does NOT auto-advance — the real, natural "sustained"
   * state a scenario stays in until the player themselves acts (owner, 2026-07-21: "я сиджу
   * допоки захочу" — sitting lasts as long as the player wants, not a fixed rep count; "встати
   * якщо сидиш, закінчити коли граєш" — a "stand up" choice while just sitting, a separate
   * "finish" choice while actively doing something). `advanceLabel` is the button shown while
   * on this step; clicking it moves to the next one. Steps without `sustain` play once and
   * auto-advance via `SkeletonAnimationViewer`'s `onComplete`. */
  sustain?: boolean;
  advanceLabel?: string;
}

interface Props {
  nodes: SkeletonNode[];
  steps: ScenarioStepTracks[];
  playing: boolean;
  skinnedMesh?: SkinnedMeshData | null;
  objUrl?: string | null;
  diffuseUrl?: string | null;
  normalUrl?: string | null;
  resolveTexture?: ((baseName: string) => Promise<string | null>) | null;
  showSkeleton: boolean;
  mirrorSkeleton: boolean;
  mirrorMesh: boolean;
}

/** Chains several real motion clips end to end on ONE skeleton — e.g. sit down → play flute
 * (held until dismissed) → stop → sit idle (held until dismissed) → stand up — each phase its
 * own real `.xmot` clip (see risenlab-inventory-scenario-idea memory: the game's own clip names
 * already encode this as a state chain, `Hero_<FromState>_..._<Action>_<Phase>` — Begin/Loop/
 * End/Ambient — no new file format needed, just playing real clips in the right order).
 *
 * Remounts `SkeletonAnimationViewer` per step (`key={stepIndex}`) rather than hot-swapping
 * tracks inside one persistent mount — simpler, and a handful of remounts across one short
 * sequence is nowhere near the rapid-fire remount volume that caused real WebGL-context
 * exhaustion elsewhere in this app (AiCompare/Model3DViewer's own documented incident); the
 * camera reframes to the same skeleton bounds each time anyway, so the reset is barely visible.
 * Holds on the final step's pose once the sequence finishes — a real one-time interaction (sit,
 * play, get up), not a looping showcase. */
export default function ScenarioPlayer({
  nodes,
  steps,
  playing,
  skinnedMesh,
  objUrl,
  diffuseUrl,
  normalUrl,
  resolveTexture,
  showSkeleton,
  mirrorSkeleton,
  mirrorMesh,
}: Props) {
  const [stepIndex, setStepIndex] = useState(0);

  // A newly selected scenario must restart from the top, not keep whatever index the previous
  // one happened to be on. The caller is expected to give `steps` a stable identity (e.g. via
  // React state set once per successful load) so this doesn't refire on unrelated re-renders.
  useEffect(() => {
    setStepIndex(0);
  }, [steps]);

  const step = steps[stepIndex];
  if (!step) return null;
  const isLastStep = stepIndex === steps.length - 1;

  function advance() {
    setStepIndex((i) => Math.min(i + 1, steps.length - 1));
  }

  return (
    <div style={{ width: "100%", height: "100%", position: "relative" }}>
      <SkeletonAnimationViewer
        key={stepIndex}
        nodes={nodes}
        tracks={step.tracks}
        playing={playing}
        loop={!!step.sustain}
        onComplete={step.sustain ? undefined : advance}
        skinnedMesh={skinnedMesh}
        objUrl={objUrl}
        diffuseUrl={diffuseUrl}
        normalUrl={normalUrl}
        resolveTexture={resolveTexture}
        showSkeleton={showSkeleton}
        mirrorSkeleton={mirrorSkeleton}
        mirrorMesh={mirrorMesh}
      />
      <div
        style={{
          position: "absolute",
          bottom: 12,
          left: 12,
          padding: "6px 12px",
          borderRadius: 8,
          background: "rgba(0,0,0,.55)",
          font: "600 11px system-ui",
          color: "#fff",
        }}
      >
        {step.label} ({stepIndex + 1}/{steps.length})
      </div>
      {step.sustain && !isLastStep ? (
        <button
          onClick={advance}
          style={{
            position: "absolute",
            bottom: 12,
            right: 12,
            padding: "9px 16px",
            borderRadius: 9,
            background: "var(--accent)",
            border: "none",
            font: "700 12.5px system-ui",
            color: "#fff",
            cursor: "pointer",
          }}
        >
          {step.advanceLabel ?? "▶"}
        </button>
      ) : null}
    </div>
  );
}
