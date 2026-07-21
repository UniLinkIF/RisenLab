import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { BoneMotion, SkeletonNode, SkinnedMeshData } from "../lib/types";
import SkeletonAnimationViewer from "./SkeletonAnimationViewer";

export interface ScenarioStepTracks {
  label: string;
  tracks: BoneMotion[];
  /** True = this step loops forever and does NOT auto-advance — the real, natural "sustained"
   * state a scenario stays in until the player themselves acts (owner, 2026-07-21: "я сиджу
   * допоки захочу" — sitting lasts as long as the player wants, not a fixed rep count).
   * `advanceLabel` is the button shown while on this step; clicking it moves to the next one,
   * which then auto-plays through any remaining one-shot steps (end the activity, stand back
   * up) on its own — the owner explicitly asked for ONE dismiss point, not a second "stand up"
   * click ("не треба закінчити і встати... це вже буде перебор"). Steps without `sustain` play
   * once and auto-advance on their own. */
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
 * (held until dismissed) → stop → stand up — each phase its own real `.xmot` clip (see
 * risenlab-inventory-scenario-idea memory: the game's own clip names already encode this as a
 * state chain, `Hero_<FromState>_..._<Action>_<Phase>` — Begin/Loop/End — no new file format
 * needed, just playing real clips in the right order).
 *
 * Mounts `SkeletonAnimationViewer` ONCE for the whole scenario and swaps the active clip via its
 * `activeClip` prop as steps advance — NOT a remount per step. A real bug (owner report,
 * 2026-07-21) with the first version (which DID remount per step, `key={stepIndex}`): every
 * transition visibly jumped/reset the camera, because sitting vs. standing bounding boxes
 * reframe differently and each remount re-ran the camera-fit logic from scratch. Swapping tracks
 * in place keeps the same camera/scene alive throughout — see `SkeletonAnimationViewer`'s
 * `activeClip` doc comment for the mechanism. Holds on the final step's pose once the sequence
 * finishes — a real one-time interaction (sit, play, get up), not a looping showcase. */
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
  // The FIRST step's tracks, frozen for this scenario's whole lifetime — seeds
  // SkeletonAnimationViewer's required `tracks` prop for its very first mounted frame, before
  // `activeClip` takes over. Never updated after mount (this component itself is remounted via
  // a `key` on the scenario's own identity whenever a genuinely different scenario is picked —
  // see Animations.tsx — so re-freezing per scenario is automatic).
  const initialTracks = useRef(steps[0]?.tracks ?? []).current;

  useEffect(() => {
    setStepIndex(0);
  }, [steps]);

  const step = steps[stepIndex];
  const isLastStep = stepIndex === steps.length - 1;

  // Stable identities across incidental re-renders (a parent prop like `resolveTexture` changing
  // reference for unrelated reasons) — only change when the step actually does, matching
  // `SkeletonAnimationViewer`'s `activeClip` effect dependency exactly. Without this, a plain
  // object/closure literal recreated every render would reset the clip's clock on EVERY
  // Animations.tsx re-render, not just real step transitions.
  const activeClip = useMemo(() => (step ? { tracks: step.tracks, sustain: step.sustain } : null), [step]);
  const advance = useCallback(() => {
    setStepIndex((i) => Math.min(i + 1, steps.length - 1));
  }, [steps.length]);

  if (!step) return null;

  return (
    <div style={{ width: "100%", height: "100%", position: "relative" }}>
      <SkeletonAnimationViewer
        nodes={nodes}
        tracks={initialTracks}
        activeClip={activeClip}
        onActiveClipComplete={step.sustain ? undefined : advance}
        playing={playing}
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
