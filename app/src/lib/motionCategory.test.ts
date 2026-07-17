import { describe, expect, it } from "vitest";
import { motionCategory } from "./motionCategory";

// Every example below is a REAL clip name from the game's animations.pak.
describe("motionCategory", () => {
  it("classifies movement", () => {
    expect(motionCategory("Ogre_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_150._xmot")).toBe("move");
    expect(motionCategory("Ogre_Stand_None_None_P0_Move_Run_N_Fwd_00_%_00_P0_300._xmot")).toBe("move");
    expect(motionCategory("Wolf_Stand_None_Fist_S0_JumpRight_00_N_Fwd_00_%_00_P0_0._xmot")).toBe("move");
    expect(motionCategory("Nautilus_Sneak_None_None_P0_Move_Walk_N_Right_00_%_00_P0_400._xmot")).toBe("move");
    expect(motionCategory("Hero_Stand_None_None_P0_Move_Turn90_N_Left_00_%_00_P0_0._xmot")).toBe("move");
  });

  it("classifies combat, including attacks that contain reaction words", () => {
    expect(motionCategory("Wolf_Stand_None_Fist_S1_StormAttack_00_N_Fwd_00_%_00_P0_0._xmot")).toBe("combat");
    expect(motionCategory("Titan_Stand_None_None_P0_SlideAttack1_Hit_N_Fwd_00_%_00_P0_0._xmot")).toBe("combat");
    expect(motionCategory("Thundertail_Stand_None_None_P0_Parade_Begin_N_Fwd_00_%_00_P0_0._xmot")).toBe("combat");
    expect(motionCategory("Wolf_Stand_None_None_P0_Warn_Begin_N_Fwd_00_%_00_P0_0._xmot")).toBe("combat");
    expect(motionCategory("Wolf_Stand_None_None_P0_Roar_Begin_N_Fwd_00_%_00_P0_0._xmot")).toBe("combat");
    // Titan boss abilities
    expect(motionCategory("Titan_Stand_None_None_P0_Flamethrower1_Loop_N_Right_00_%_00_P0_0._xmot")).toBe("combat");
    expect(motionCategory("Titan_Stand_None_None_P0_Stomp_Begin_N_Fwd_00_%_00_P0_0._xmot")).toBe("combat");
    expect(motionCategory("Titan_Stand_None_None_P0_FinishHim1_Kill_N_Fwd_00_%_00_P0_0._xmot")).toBe("combat");
  });

  it("classifies reactions", () => {
    expect(motionCategory("Wolf_Stand_None_None_P0_Stumble_00_N_Fwd_00_%_00_P0_0._xmot")).toBe("react");
    expect(motionCategory("Wolf_Stand_None_None_S0_StumbleBack_00_N_Fwd_00_%_00_P0_0._xmot")).toBe("react");
    expect(motionCategory("Lizard_Stand_None_None_P0_Dead_Begin_N_Fwd_00_%_00_P0_0._xmot")).toBe("react");
    expect(motionCategory("BridgeFall._xmot")).toBe("react");
  });

  it("classifies idle/daily-life", () => {
    expect(motionCategory("Wolf_Stand_None_None_P0_Ambient_Loop_N_Fwd_00_%_00_P0_0._xmot")).toBe("idle");
    expect(motionCategory("Wolf_Stand_None_None_P0_SleepGround_Begin_N_Fwd_00_%_00_P0_0._xmot")).toBe("idle");
    expect(motionCategory("Ogre_Stand_None_None_P0_Eat_Loop_N_Fwd_00_%_00_P0_0._xmot")).toBe("idle");
    expect(motionCategory("Hero_Stand_None_None_P0_Listen_Loop_N_Fwd_00_%_00_P0_0._xmot")).toBe("idle");
  });

  it("leaves interaction/unknown clips in other", () => {
    expect(motionCategory("Hero_Stand_None_None_P0_LockPick_Loop_N_Fwd_00_%_00_P0_0._xmot")).toBe("other");
    expect(motionCategory("Hero_Stand_None_None_P0_OpenChest_Begin_N_Fwd_00_%_00_P0_0._xmot")).toBe("other");
  });
});
