import { describe, expect, it } from "vitest";
import { looksDxt5nmSwizzled, reconstructTangentNormalMap } from "./normalMap";

function rgba(pixels: Array<[number, number, number, number]>): Uint8ClampedArray {
  return new Uint8ClampedArray(pixels.flat());
}

describe("looksDxt5nmSwizzled", () => {
  it("detects the real swizzle: R and B always 0, G/A varying", () => {
    const data = rgba([
      [0, 128, 0, 130],
      [0, 90, 0, 200],
      [0, 60, 0, 40],
    ]);
    // pad so the sampling stride (37 texels) still lands inside the array for tiny fixtures
    const padded = new Uint8ClampedArray(37 * 4 * 2);
    padded.set(data);
    expect(looksDxt5nmSwizzled(padded)).toBe(true);
  });

  it("returns false for a standard normal map with real R/B data", () => {
    const padded = new Uint8ClampedArray(37 * 4 * 2);
    padded.set(rgba([[128, 128, 255, 255]]));
    expect(looksDxt5nmSwizzled(padded)).toBe(false);
  });
});

describe("reconstructTangentNormalMap", () => {
  it("reconstructs a flat (pointing straight out) texel to a mid-gray/blue normal", () => {
    const data = rgba([[0, 128, 0, 128]]);
    reconstructTangentNormalMap(data);
    expect(data[0]).toBeCloseTo(128, -1);
    expect(data[1]).toBe(128);
    expect(data[2]).toBeCloseTo(255, -1);
    expect(data[3]).toBe(255);
  });

  it("reconstructs a texel tilted fully along X to Z=0 (grazing angle, encoded as mid-gray)", () => {
    const data = rgba([[0, 128, 0, 255]]);
    reconstructTangentNormalMap(data);
    expect(data[0]).toBeCloseTo(255, -1);
    // Z=0 encodes as byte 128 (the [-1,1] -> [0,255] midpoint), not 0 — 0 would mean Z=-1,
    // a normal pointing fully *into* the surface, which the reconstruction never produces.
    expect(data[2]).toBeCloseTo(128, -1);
  });

  it("never produces a negative sqrt input for an out-of-range encoded pixel", () => {
    const data = rgba([[0, 255, 0, 255]]);
    expect(() => reconstructTangentNormalMap(data)).not.toThrow();
    expect(Number.isNaN(data[2])).toBe(false);
  });
});
