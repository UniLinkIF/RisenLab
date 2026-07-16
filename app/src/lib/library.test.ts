import { describe, expect, it } from "vitest";
import { badgeForStatus, buildFolderTree, countProcessed, entryTreeKey, filterByTreeKey, filterEntries, findTextureByBaseName, formatBytes, isFlat2DOnly } from "./library";
import type { LibraryEntry, ReviewStatus } from "./types";

function entry(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return {
    group: "compiled",
    archivePath: "C:/Game/data/compiled/images.pak",
    archiveStem: "images",
    entryPath: "Level/Nat_Rock/Nat_Stone_Rock_01._ximg",
    pngRel: "compiled/images/Level/Nat_Rock/Nat_Stone_Rock_01.png",
    name: "Nat_Stone_Rock_01._ximg",
    folder: "Level/Nat_Rock",
    ...overrides,
  };
}

describe("entryTreeKey", () => {
  it("joins group/archive/full folder path, not just the top segment", () => {
    expect(entryTreeKey(entry())).toBe("compiled/images/Level/Nat_Rock");
  });

  it("uses (root) for entries with no folder", () => {
    expect(entryTreeKey(entry({ folder: "" }))).toBe("compiled/images/(root)");
  });

  it("tolerates a leading slash like real archive entry paths produce", () => {
    // Real Risen archives report entry paths with a leading "/" (e.g. "/Animation/Monster/x"),
    // which historically made every entry collapse into one blank top-level folder — see
    // batch.rs's matching regression test.
    expect(entryTreeKey(entry({ folder: "/Animation/Monster" }))).toBe("compiled/images/Animation/Monster");
  });
});

describe("buildFolderTree", () => {
  it("produces group -> archive -> folder nodes with correct counts", () => {
    const entries = [
      entry(),
      entry({ name: "b._ximg", folder: "Level/Nat_Rock" }),
      entry({ name: "c._ximg", folder: "Animation" }),
      entry({ group: "common", archiveStem: "materials", folder: "Item", name: "d._ximg" }),
    ];
    const tree = buildFolderTree(entries);

    const byKey = Object.fromEntries(tree.map((n) => [n.key, n]));
    expect(byKey["compiled"].count).toBe(3);
    expect(byKey["compiled/images"].count).toBe(3);
    expect(byKey["compiled/images/Level"].count).toBe(2);
    expect(byKey["compiled/images/Animation"].count).toBe(1);
    expect(byKey["common"].count).toBe(1);
    expect(byKey["common/materials/Item"].count).toBe(1);

    // groups sorted, and depth is correct for every node
    expect(tree.map((n) => n.depth)).toEqual(tree.map((n) => n.key.split("/").length - 1));
  });

  it("returns an empty tree for no entries", () => {
    expect(buildFolderTree([])).toEqual([]);
  });

  it("recurses into nested subfolders instead of collapsing them into the top segment", () => {
    // Real game data has folders like Animation/Monster, Animation/Player — before the fix
    // these both collapsed into a single "Animation" leaf, hiding the real breakdown.
    const entries = [
      entry({ folder: "Animation/Monster", name: "a._ximg" }),
      entry({ folder: "Animation/Monster", name: "b._ximg" }),
      entry({ folder: "Animation/Player", name: "c._ximg" }),
    ];
    const tree = buildFolderTree(entries);
    const byKey = Object.fromEntries(tree.map((n) => [n.key, n]));

    expect(byKey["compiled/images/Animation"].count).toBe(3);
    expect(byKey["compiled/images/Animation/Monster"].count).toBe(2);
    expect(byKey["compiled/images/Animation/Player"].count).toBe(1);
    expect(byKey["compiled/images/Animation/Monster"].depth).toBe(3);
  });

  it("strips a leading slash so real archive entries don't collapse into one blank folder", () => {
    const entries = [entry({ folder: "/Speedtree", name: "a._ximg" })];
    const tree = buildFolderTree(entries);
    expect(tree.some((n) => n.key === "compiled/images/Speedtree" && n.label === "Speedtree")).toBe(true);
  });
});

describe("filterByTreeKey", () => {
  const entries = [
    entry(),
    entry({ name: "b._ximg", folder: "Animation" }),
    entry({ group: "common", archiveStem: "materials", folder: "Item", name: "c._ximg" }),
  ];

  it("returns everything when key is null", () => {
    expect(filterByTreeKey(entries, null)).toHaveLength(3);
  });

  it("filters to an exact leaf key", () => {
    const result = filterByTreeKey(entries, "compiled/images/Level");
    expect(result).toHaveLength(1);
    expect(result[0].name).toBe("Nat_Stone_Rock_01._ximg");
  });

  it("filters to a group-level key, including all nested folders", () => {
    const result = filterByTreeKey(entries, "compiled");
    expect(result).toHaveLength(2);
  });

  it("does not prefix-match a different archive/folder that merely starts with the same characters", () => {
    const withDecoy = [...entries, entry({ archiveStem: "images2", name: "decoy._ximg" })];
    const result = filterByTreeKey(withDecoy, "compiled/images");
    expect(result.every((e) => e.archiveStem === "images")).toBe(true);
  });
});

describe("filterEntries", () => {
  const entries = [entry(), entry({ name: "Bark_Oak._ximg", folder: "Level/Trees", archiveStem: "images" })];

  it("returns everything for an empty query", () => {
    expect(filterEntries(entries, "  ")).toHaveLength(2);
  });

  it("matches case-insensitively on name", () => {
    expect(filterEntries(entries, "bark")).toHaveLength(1);
  });

  it("matches on folder", () => {
    expect(filterEntries(entries, "trees")).toHaveLength(1);
  });

  it("matches on archive stem", () => {
    expect(filterEntries(entries, "IMAGES")).toHaveLength(2);
  });

  it("returns nothing when nothing matches", () => {
    expect(filterEntries(entries, "nonexistent")).toHaveLength(0);
  });
});

describe("findTextureByBaseName", () => {
  const entries = [
    entry({ name: "ItWpn_SwordBlades_01_Diffuse_01._ximg" }),
    entry({ name: "ItWpn_SwordBlades_01_Normal_01._ximg" }),
  ];

  it("matches the real library entry despite the different dev-time extension", () => {
    const match = findTextureByBaseName(entries, "ItWpn_SwordBlades_01_Diffuse_01.tga");
    expect(match?.name).toBe("ItWpn_SwordBlades_01_Diffuse_01._ximg");
  });

  it("matches case-insensitively", () => {
    const match = findTextureByBaseName(entries, "itwpn_swordblades_01_normal_01.TGA");
    expect(match?.name).toBe("ItWpn_SwordBlades_01_Normal_01._ximg");
  });

  it("returns null when nothing matches", () => {
    expect(findTextureByBaseName(entries, "SomethingElse.tga")).toBeNull();
  });
});

describe("isFlat2DOnly", () => {
  it("flags known flat-2D folders (GUI, editor icons, lightmaps, debug assets)", () => {
    expect(isFlat2DOnly(entry({ folder: "GUI" }))).toBe(true);
    expect(isFlat2DOnly(entry({ folder: "EditSupporter/Icons" }))).toBe(true);
    expect(isFlat2DOnly(entry({ folder: "lightmaps" }))).toBe(true);
    expect(isFlat2DOnly(entry({ folder: "Testkram_FinalBattle" }))).toBe(false); // not an exact top-segment match
    expect(isFlat2DOnly(entry({ folder: "Testkram" }))).toBe(true);
  });

  it("does not flag real material folders even when their contents look generic", () => {
    // Real weapon diffuse/normal maps live under "Special" in the actual game archive.
    expect(isFlat2DOnly(entry({ folder: "Special" }))).toBe(false);
    expect(isFlat2DOnly(entry({ folder: "Level/Nat_Rock" }))).toBe(false);
    expect(isFlat2DOnly(entry({ folder: "Animation/Monster" }))).toBe(false);
  });

  it("does not flag entries with no folder", () => {
    expect(isFlat2DOnly(entry({ folder: "" }))).toBe(false);
  });
});

describe("formatBytes", () => {
  it("formats sub-1KB sizes as bytes", () => {
    expect(formatBytes(512)).toBe("512 B");
  });

  it("formats KB/MB/GB with one decimal below 10 units", () => {
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
  });

  it("drops the decimal at 10 or more units", () => {
    expect(formatBytes(12 * 1024)).toBe("12 KB");
  });

  it("formats a real-world archive size (590MB) sensibly", () => {
    expect(formatBytes(590 * 1024 * 1024)).toBe("590 MB");
  });
});

describe("badgeForStatus", () => {
  it("returns null for untouched (no status) entries", () => {
    expect(badgeForStatus(undefined, "uk")).toBeNull();
  });

  it("returns an accent AI badge for pending review", () => {
    expect(badgeForStatus("pending", "uk")).toEqual({ label: "AI", background: "var(--accent)" });
  });

  it("returns a green done badge for approved, localized", () => {
    expect(badgeForStatus("approved", "uk")?.label).toBe("ГОТОВО");
    expect(badgeForStatus("approved", "en")?.label).toBe("DONE");
  });

  it("returns null for rejected (reverted to untouched)", () => {
    expect(badgeForStatus("rejected", "uk")).toBeNull();
  });
});

describe("countProcessed", () => {
  const entries = [entry({ name: "a" }), entry({ name: "b", pngRel: "b.png" }), entry({ name: "c", pngRel: "c.png" })];

  it("counts only entries present in the status map", () => {
    const status = new Map<string, ReviewStatus>([
      [entries[0].pngRel, "approved"],
      [entries[1].pngRel, "pending"],
    ]);
    expect(countProcessed(entries, status)).toBe(2);
  });

  it("returns 0 when nothing has been processed", () => {
    expect(countProcessed(entries, new Map())).toBe(0);
  });

  it("does not count entries outside the given list even if present in the map", () => {
    const status = new Map<string, ReviewStatus>([["unrelated.png", "approved"]]);
    expect(countProcessed(entries, status)).toBe(0);
  });
});
