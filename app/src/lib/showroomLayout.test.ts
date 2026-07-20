import { describe, expect, it } from "vitest";
import { gridPositions, gridRowCount, normalizeScale, rowPositions, stackZones } from "./showroomLayout";

describe("gridPositions", () => {
  it("fills columns left to right before starting a new row", () => {
    const positions = gridPositions({ count: 5, columns: 3, cellSize: 1, origin: [0, 0, 0], axis: "floor" });
    expect(positions[0]).toEqual([0, 0, 0]);
    expect(positions[2]).toEqual([2, 0, 0]);
    expect(positions[3]).toEqual([0, 0, 1]); // wraps to row 1
    expect(positions).toHaveLength(5);
  });

  it("wall grids step DOWN (-Y) per row, floor grids step AWAY (+Z) per row", () => {
    const wall = gridPositions({ count: 4, columns: 2, cellSize: 2, origin: [0, 10, 0], axis: "wall" });
    expect(wall[2]).toEqual([0, 8, 0]); // row 1 of the wall is 2 units lower
    const floor = gridPositions({ count: 4, columns: 2, cellSize: 2, origin: [0, 0, 0], axis: "floor" });
    expect(floor[2]).toEqual([0, 0, 2]); // row 1 of the floor is 2 units further away
  });

  it("clamps a zero/negative column count to 1 instead of dividing by zero", () => {
    const positions = gridPositions({ count: 3, columns: 0, cellSize: 1, origin: [0, 0, 0], axis: "floor" });
    expect(positions).toHaveLength(3);
    expect(positions.every((p) => Number.isFinite(p[0]) && Number.isFinite(p[2]))).toBe(true);
  });
});

describe("gridRowCount", () => {
  it("rounds up to a whole row", () => {
    expect(gridRowCount(10, 4)).toBe(3);
    expect(gridRowCount(8, 4)).toBe(2);
    expect(gridRowCount(1, 4)).toBe(1);
  });
});

describe("rowPositions", () => {
  it("centers the row on its origin rather than starting at it", () => {
    const positions = rowPositions({ count: 3, spacing: 2, origin: [0, 0, 0], axis: "x" });
    expect(positions).toEqual([
      [-2, 0, 0],
      [0, 0, 0],
      [2, 0, 0],
    ]);
  });

  it("a single-item row sits exactly on the origin", () => {
    const positions = rowPositions({ count: 1, spacing: 5, origin: [3, 0, 7], axis: "z" });
    expect(positions).toEqual([[3, 0, 7]]);
  });

  it("axis z runs the row in depth, not sideways", () => {
    const positions = rowPositions({ count: 2, spacing: 4, origin: [0, 0, 0], axis: "z" });
    expect(positions[0][0]).toBe(0); // x unchanged
    expect(positions[0][2]).not.toBe(positions[1][2]); // z varies
  });
});

describe("stackZones", () => {
  it("places each zone after the previous one's full depth plus the gap", () => {
    const origins = stackZones(
      [
        { id: "a", depth: 10 },
        { id: "b", depth: 5 },
        { id: "c", depth: 0 },
      ],
      0,
      2,
    );
    expect(origins.a).toBe(0);
    expect(origins.b).toBe(12); // 0 + 10 + gap 2
    expect(origins.c).toBe(19); // 12 + 5 + gap 2
  });

  it("starts from the given startZ, not always 0", () => {
    const origins = stackZones([{ id: "a", depth: 3 }], 100, 1);
    expect(origins.a).toBe(100);
  });

  it("a zero-depth zone still gets its own slot instead of collapsing into its neighbor", () => {
    const origins = stackZones(
      [
        { id: "a", depth: 0 },
        { id: "b", depth: 0 },
      ],
      0,
      5,
    );
    expect(origins.a).not.toBe(origins.b);
  });
});

describe("normalizeScale", () => {
  it("scales the longest axis to exactly the target size", () => {
    const scale = normalizeScale([2, 4, 1], 8);
    expect(scale).toBeCloseTo(2, 10); // 4 * 2 = 8
  });

  it("picks whichever axis is actually longest, not always the first", () => {
    const scaleX = normalizeScale([10, 1, 1], 5);
    const scaleZ = normalizeScale([1, 1, 10], 5);
    expect(scaleX).toBeCloseTo(scaleZ, 10);
  });

  it("falls back to 1 for a degenerate (zero-size) bounding box instead of dividing by zero", () => {
    const scale = normalizeScale([0, 0, 0], 5);
    expect(Number.isFinite(scale)).toBe(true);
    expect(scale).toBe(1);
  });
});
