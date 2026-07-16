import { describe, expect, it } from "vitest";
import { computeFraming } from "./framing";

describe("computeFraming", () => {
  it("scales fit distance with the largest dimension", () => {
    const small = computeFraming({ x: 1, y: 1, z: 1 }, 45);
    const large = computeFraming({ x: 10, y: 10, z: 10 }, 45);
    expect(large.fitDistance).toBeCloseTo(small.fitDistance * 10, 5);
  });

  it("picks the max of x/y/z as the driving dimension", () => {
    const a = computeFraming({ x: 5, y: 1, z: 1 }, 45);
    const b = computeFraming({ x: 1, y: 5, z: 1 }, 45);
    const c = computeFraming({ x: 1, y: 1, z: 5 }, 45);
    expect(a.fitDistance).toBeCloseTo(b.fitDistance, 5);
    expect(b.fitDistance).toBeCloseTo(c.fitDistance, 5);
  });

  it("never divides by zero for a degenerate (all-zero) size", () => {
    const f = computeFraming({ x: 0, y: 0, z: 0 }, 45);
    expect(Number.isFinite(f.fitDistance)).toBe(true);
    expect(f.fitDistance).toBeGreaterThan(0);
  });

  it("derives near/far clip planes proportional to model scale", () => {
    const f = computeFraming({ x: 100, y: 10, z: 10 }, 45);
    expect(f.near).toBeCloseTo(1, 5);
    expect(f.far).toBeCloseTo(10000, 5);
  });

  it("a wider field of view yields a smaller fit distance for the same model", () => {
    const narrow = computeFraming({ x: 4, y: 4, z: 4 }, 30);
    const wide = computeFraming({ x: 4, y: 4, z: 4 }, 90);
    expect(wide.fitDistance).toBeLessThan(narrow.fitDistance);
  });
});
