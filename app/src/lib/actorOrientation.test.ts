import { describe, expect, it } from "vitest";
import { getActorOrientation, orientationKey, setActorOrientation, type KeyValueStorage } from "./actorOrientation";

function fakeStorage(): KeyValueStorage {
  const map = new Map<string, string>();
  return {
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => {
      map.set(key, value);
    },
  };
}

describe("orientationKey", () => {
  it("keys by archive file name only, not the full absolute path", () => {
    const a = orientationKey("C:/games/risen/data/compiled/animations.pak", "/foo/Bar._xmac");
    const b = orientationKey("D:/elsewhere/animations.pak", "/foo/Bar._xmac");
    expect(a).toBe(b);
    expect(a).toBe("animations.pak::/foo/Bar._xmac");
  });
});

describe("getActorOrientation / setActorOrientation", () => {
  it("defaults an unknown actor to identity on both flags", () => {
    const storage = fakeStorage();
    expect(getActorOrientation("C:/x/animations.pak", "/Unknown._xmac", storage)).toEqual({
      mirrorSkeleton: false,
      mirrorMesh: false,
    });
  });

  it("uses the real owner-confirmed seed for Wolf (identity) and Pig (both mirrored)", () => {
    const storage = fakeStorage();
    expect(getActorOrientation("C:/x/animations.pak", "/_emfx36/Monster/Bodys/Ani_Wolf_Monster_Wolf._xmac", storage)).toEqual({
      mirrorSkeleton: false,
      mirrorMesh: false,
    });
    expect(getActorOrientation("C:/x/animations.pak", "/_emfx36/Monster/Bodys/Ani_Pig_Monster_Pig._xmac", storage)).toEqual({
      mirrorSkeleton: true,
      mirrorMesh: true,
    });
  });

  it("persists a manual override and returns it on the next lookup", () => {
    const storage = fakeStorage();
    setActorOrientation("C:/x/animations.pak", "/Foo/Bar._xmac", { mirrorSkeleton: true, mirrorMesh: false }, storage);
    expect(getActorOrientation("C:/x/animations.pak", "/Foo/Bar._xmac", storage)).toEqual({
      mirrorSkeleton: true,
      mirrorMesh: false,
    });
  });

  it("a manual override replaces the seeded default", () => {
    const storage = fakeStorage();
    setActorOrientation(
      "C:/x/animations.pak",
      "/_emfx36/Monster/Bodys/Ani_Wolf_Monster_Wolf._xmac",
      { mirrorSkeleton: true, mirrorMesh: true },
      storage,
    );
    expect(getActorOrientation("C:/x/animations.pak", "/_emfx36/Monster/Bodys/Ani_Wolf_Monster_Wolf._xmac", storage)).toEqual({
      mirrorSkeleton: true,
      mirrorMesh: true,
    });
  });

  it("allows skeleton and mesh to be flipped independently", () => {
    const storage = fakeStorage();
    setActorOrientation("C:/x/animations.pak", "/A._xmac", { mirrorSkeleton: true, mirrorMesh: false }, storage);
    expect(getActorOrientation("C:/x/animations.pak", "/A._xmac", storage)).toEqual({ mirrorSkeleton: true, mirrorMesh: false });
  });

  it("keeps overrides for different actors independent", () => {
    const storage = fakeStorage();
    setActorOrientation("C:/x/animations.pak", "/A._xmac", { mirrorSkeleton: true, mirrorMesh: true }, storage);
    setActorOrientation("C:/x/animations.pak", "/B._xmac", { mirrorSkeleton: false, mirrorMesh: false }, storage);
    expect(getActorOrientation("C:/x/animations.pak", "/A._xmac", storage)).toEqual({ mirrorSkeleton: true, mirrorMesh: true });
    expect(getActorOrientation("C:/x/animations.pak", "/B._xmac", storage)).toEqual({ mirrorSkeleton: false, mirrorMesh: false });
  });
});
