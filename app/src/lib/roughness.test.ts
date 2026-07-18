import { describe, expect, it } from "vitest";
import { specularLuminanceToRoughness } from "./roughness";

function rgba(pixels: Array<[number, number, number, number]>): Uint8ClampedArray {
  return new Uint8ClampedArray(pixels.flat());
}

describe("specularLuminanceToRoughness", () => {
  it("maps a bright (shiny) specular texel to a LOW roughness byte", () => {
    const data = rgba([[255, 255, 255, 255]]);
    specularLuminanceToRoughness(data);
    expect(data[0]).toBeLessThan(60);
    expect(data[0]).toBe(data[1]);
    expect(data[1]).toBe(data[2]);
    expect(data[3]).toBe(255);
  });

  it("maps a dark (matte) specular texel to a HIGH roughness byte", () => {
    const data = rgba([[0, 0, 0, 255]]);
    specularLuminanceToRoughness(data);
    expect(data[0]).toBeGreaterThan(200);
  });

  it("never reaches the literal 0/255 extremes (clamped range)", () => {
    const bright = rgba([[255, 255, 255, 255]]);
    specularLuminanceToRoughness(bright);
    expect(bright[0]).toBeGreaterThan(0);
    const dark = rgba([[0, 0, 0, 255]]);
    specularLuminanceToRoughness(dark);
    expect(dark[0]).toBeLessThan(255);
  });

  it("is monotonic: brighter specular input never produces a higher roughness output", () => {
    const dim = rgba([[80, 80, 80, 255]]);
    const bright = rgba([[200, 200, 200, 255]]);
    specularLuminanceToRoughness(dim);
    specularLuminanceToRoughness(bright);
    expect(bright[0]).toBeLessThan(dim[0]);
  });
});
