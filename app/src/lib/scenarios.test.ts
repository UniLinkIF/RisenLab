import { describe, expect, it } from "vitest";
import { deriveScenarios, matchScenariosForItemName } from "./scenarios";
import type { MotionEntry } from "./types";

const INTERACTS = "_emfx36/Humans/Animations/Interacts";

function clip(name: string, folder = INTERACTS): MotionEntry {
  return { group: "compiled", archivePath: "animations.pak", archiveStem: "animations", entryPath: `/${folder}/${name}`, name, folder };
}

describe("deriveScenarios", () => {
  it("builds the full sit-down/activity/stand-up chain for a real sitting scenario", () => {
    const motions = [
      clip("Hero_Stand_None_None_P0_SitGround_Begin_N_Fwd_00_%_00_P0_0._xmot"),
      clip("Hero_SitGround_None_None_P0_Stand_Begin_N_Fwd_00_%_00_P0_0._xmot"),
      clip("Hero_SitGround_None_None_P0_PlayFlute_Begin_N_Fwd_00_%_00_P0_0._xmot"),
      clip("Hero_SitGround_None_None_P0_PlayFlute_Loop_N_Fwd_00_%_00_P0_0._xmot"),
      clip("Hero_SitGround_None_None_P0_PlayFlute_End_N_Fwd_00_%_00_P0_0._xmot"),
    ];
    const scenarios = deriveScenarios(motions);
    expect(scenarios).toHaveLength(1);
    const s = scenarios[0];
    expect(s.label).toBe("🎭 Play Flute (Sit Ground)");
    expect(s.clips.map((c) => c.name)).toEqual([
      "Hero_Stand_None_None_P0_SitGround_Begin_N_Fwd_00_%_00_P0_0._xmot",
      "Hero_SitGround_None_None_P0_PlayFlute_Begin_N_Fwd_00_%_00_P0_0._xmot",
      "Hero_SitGround_None_None_P0_PlayFlute_Loop_N_Fwd_00_%_00_P0_0._xmot",
      "Hero_SitGround_None_None_P0_PlayFlute_End_N_Fwd_00_%_00_P0_0._xmot",
      "Hero_SitGround_None_None_P0_Stand_Begin_N_Fwd_00_%_00_P0_0._xmot",
    ]);
    // Only the Loop phase holds until dismissed — owner correction (2026-07-21): one dismiss
    // point per scenario, not a separate "finish" + "stand up" pair.
    expect(s.clips.filter((c) => c.sustain)).toHaveLength(1);
    expect(s.clips.find((c) => c.sustain)?.advanceLabel).toBe("⏹ Закінчити");
  });

  it("skips the enter/exit transition steps for a standing activity", () => {
    const motions = [
      clip("Hero_Stand_None_None_P0_PlayGuitar_Begin_N_Fwd_00_%_00_P0_0._xmot"),
      clip("Hero_Stand_None_None_P0_PlayGuitar_Loop_N_Fwd_00_%_00_P0_0._xmot"),
    ];
    const scenarios = deriveScenarios(motions);
    expect(scenarios).toHaveLength(1);
    expect(scenarios[0].label).toBe("🎭 Play Guitar");
    expect(scenarios[0].clips.map((c) => c.name)).toEqual([
      "Hero_Stand_None_None_P0_PlayGuitar_Begin_N_Fwd_00_%_00_P0_0._xmot",
      "Hero_Stand_None_None_P0_PlayGuitar_Loop_N_Fwd_00_%_00_P0_0._xmot",
    ]);
  });

  it("never turns a pure state transition into its own scenario", () => {
    const motions = [
      clip("Hero_Stand_None_None_P0_SitStool_Begin_N_Fwd_00_%_00_P0_0._xmot"),
      clip("Hero_SitStool_None_None_P0_Stand_Begin_N_Fwd_00_%_00_P0_0._xmot"),
    ];
    expect(deriveScenarios(motions)).toEqual([]);
  });

  it("excludes hand-pose overlay clips (HoldRight) from the scenario list", () => {
    const motions = [clip("Hero_SitGround_None_Flute_P0_HoldRight_Begin_O_Fwd_00_%_00_P0_0._xmot")];
    expect(deriveScenarios(motions)).toEqual([]);
  });

  it("ignores clips outside the real Interacts folder even with the same name shape", () => {
    const motions = [clip("Hero_Stand_None_None_P0_Walk_Loop_N_Fwd_00_%_00_P0_0._xmot", "_emfx36/Humans/Animations/Ambient")];
    expect(deriveScenarios(motions)).toEqual([]);
  });

  it("picks the plain forward, non-overlay, variant-00 clip when several direction variants exist", () => {
    const motions = [
      clip("Hero_Stand_None_None_P0_PlayGuitar_Loop_N_Left_00_%_00_P0_0._xmot"),
      clip("Hero_Stand_None_None_P0_PlayGuitar_Loop_N_Fwd_00_%_00_P0_0._xmot"),
      clip("Hero_Stand_None_None_P0_PlayGuitar_Loop_N_Right_00_%_00_P0_0._xmot"),
    ];
    const scenarios = deriveScenarios(motions);
    expect(scenarios[0].clips[0].name).toBe("Hero_Stand_None_None_P0_PlayGuitar_Loop_N_Fwd_00_%_00_P0_0._xmot");
  });
});

describe("matchScenariosForItemName", () => {
  const motions = [
    clip("Hero_Stand_None_None_P0_SitGround_Begin_N_Fwd_00_%_00_P0_0._xmot"),
    clip("Hero_SitGround_None_None_P0_Stand_Begin_N_Fwd_00_%_00_P0_0._xmot"),
    clip("Hero_SitGround_None_None_P0_PlayFlute_Begin_N_Fwd_00_%_00_P0_0._xmot"),
    clip("Hero_SitGround_None_None_P0_PlayFlute_Loop_N_Fwd_00_%_00_P0_0._xmot"),
    clip("Hero_Stand_None_None_P0_PlayGuitar_Loop_N_Fwd_00_%_00_P0_0._xmot"),
  ];
  const scenarios = deriveScenarios(motions);

  it("matches a real item name to the scenario whose action names it", () => {
    const matches = matchScenariosForItemName(scenarios, "Item_Flute._xmsh");
    expect(matches.map((s) => s.label)).toEqual(["🎭 Play Flute (Sit Ground)"]);
  });

  it("ignores noise tokens (Item_/numbers/condition words) that would otherwise false-match", () => {
    const matches = matchScenariosForItemName(scenarios, "Item_Meat_Raw_01._xmsh");
    expect(matches).toEqual([]); // no ConsumeMeat scenario in this small fixture — proves "raw"/"01" aren't matching anything spuriously
  });

  it("returns no matches for an item with no corresponding scenario — a real, informative answer", () => {
    expect(matchScenariosForItemName(scenarios, "It_Wpn_TraitorSword._xmsh")).toEqual([]);
  });
});
