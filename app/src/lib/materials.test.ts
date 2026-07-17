import { describe, expect, it } from "vitest";
import { deriveNormalName, findTextureEntryForBaseName, groupFacesByMaterial } from "./materials";

describe("deriveNormalName", () => {
  it("maps the real _Diffuse_ -> _Normal_ convention", () => {
    expect(deriveNormalName("ItWpn_Axes_01_Diffuse_01")).toBe("ItWpn_Axes_01_Normal_01");
  });

  it("handles a trailing Diffuse without underscores", () => {
    expect(deriveNormalName("Ani_Monster_Wolf_Body_01_Diffuse")).toBe("Ani_Monster_Wolf_Body_01_Normal");
  });

  it("returns null when there is no diffuse marker", () => {
    expect(deriveNormalName("EMFX_Default")).toBeNull();
  });
});

describe("findTextureEntryForBaseName", () => {
  const entries = [
    { name: "ItWpn_Axes_01_Diffuse_01._ximg" },
    { name: "Ani_Monster_Wolf_Body_01_Diffuse_S1._ximg" },
  ];

  it("finds an exact base-name match", () => {
    expect(findTextureEntryForBaseName(entries, "ItWpn_Axes_01_Diffuse_01")?.name).toBe("ItWpn_Axes_01_Diffuse_01._ximg");
  });

  it("falls back to the _S1 detail-stage suffix actor materials omit", () => {
    // Real case: the Wolf's material references "Ani_Monster_Wolf_Body_01_Diffuse" but the
    // only real texture file is "..._Diffuse_S1".
    expect(findTextureEntryForBaseName(entries, "Ani_Monster_Wolf_Body_01_Diffuse")?.name).toBe(
      "Ani_Monster_Wolf_Body_01_Diffuse_S1._ximg",
    );
  });

  it("returns null when neither exists", () => {
    expect(findTextureEntryForBaseName(entries, "Nope")).toBeNull();
  });
});

describe("groupFacesByMaterial", () => {
  it("splits interleaved faces into contiguous per-material runs (stable order)", () => {
    const faces: [number, number, number][] = [
      [0, 1, 2],
      [3, 4, 5],
      [6, 7, 8],
      [9, 10, 11],
    ];
    const { index, groups } = groupFacesByMaterial(faces, [1, 0, 1, 0]);
    expect(index).toEqual([3, 4, 5, 9, 10, 11, 0, 1, 2, 6, 7, 8]);
    expect(groups).toEqual([
      { start: 0, count: 6, materialId: 0 },
      { start: 6, count: 6, materialId: 1 },
    ]);
  });

  it("keeps a single-material mesh as one group", () => {
    const faces: [number, number, number][] = [
      [0, 1, 2],
      [3, 4, 5],
    ];
    const { index, groups } = groupFacesByMaterial(faces, [0, 0]);
    expect(index).toEqual([0, 1, 2, 3, 4, 5]);
    expect(groups).toEqual([{ start: 0, count: 6, materialId: 0 }]);
  });

  it("defaults faces without an id to material 0 instead of dropping them", () => {
    const faces: [number, number, number][] = [
      [0, 1, 2],
      [3, 4, 5],
    ];
    const { index, groups } = groupFacesByMaterial(faces, [2]);
    expect(index).toHaveLength(6);
    expect(groups.map((g) => g.materialId)).toEqual([0, 2]);
  });
});

describe("findTextureEntryForBaseName infix drift", () => {
  it("resolves the real Ogre belt case (actor says Ani_Monster_..., file is Ani_Hero_Monster_...)", () => {
    const entries = [
      { name: "Ani_Hero_Monster_Oger_Body_Diffuse_S1._ximg" },
      { name: "Ani_Hero_Monster_Oger_Cloth_Diffuse_S1._ximg" },
    ];
    expect(findTextureEntryForBaseName(entries, "Ani_Monster_Oger_Cloth_Diffuse_S1")?.name).toBe(
      "Ani_Hero_Monster_Oger_Cloth_Diffuse_S1._ximg",
    );
  });

  it("refuses ambiguous or too-short tails", () => {
    const entries = [
      { name: "A_Long_Common_Tail_Diffuse_S1._ximg" },
      { name: "B_Long_Common_Tail_Diffuse_S1._ximg" },
    ];
    expect(findTextureEntryForBaseName(entries, "X_Long_Common_Tail_Diffuse_S1")).toBeNull();
    expect(findTextureEntryForBaseName([{ name: "Whatever._ximg" }], "A_Bc")).toBeNull();
  });
});

