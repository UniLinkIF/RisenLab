import { describe, expect, it } from "vitest";
import { addManualScenario, getManualScenarioIds, removeManualScenario, type KeyValueStorage } from "./itemScenarios";

function fakeStorage(): KeyValueStorage {
  const map = new Map<string, string>();
  return {
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => {
      map.set(key, value);
    },
  };
}

describe("addManualScenario / getManualScenarioIds / removeManualScenario", () => {
  it("defaults an unknown item to no manual associations", () => {
    expect(getManualScenarioIds("Item_Unknown._xmsh", fakeStorage())).toEqual([]);
  });

  it("adds a manual association and persists it", () => {
    const storage = fakeStorage();
    addManualScenario("Item_Lute._xmsh", "Stand::::PlayGuitar", storage);
    expect(getManualScenarioIds("Item_Lute._xmsh", storage)).toEqual(["Stand::::PlayGuitar"]);
  });

  it("never adds the same scenario id twice", () => {
    const storage = fakeStorage();
    addManualScenario("Item_Lute._xmsh", "Stand::::PlayGuitar", storage);
    addManualScenario("Item_Lute._xmsh", "Stand::::PlayGuitar", storage);
    expect(getManualScenarioIds("Item_Lute._xmsh", storage)).toEqual(["Stand::::PlayGuitar"]);
  });

  it("supports multiple manual associations for the same item", () => {
    const storage = fakeStorage();
    addManualScenario("Item_Lute._xmsh", "Stand::::PlayGuitar", storage);
    addManualScenario("Item_Lute._xmsh", "SitStool::::PlayFlute", storage);
    expect(getManualScenarioIds("Item_Lute._xmsh", storage)).toEqual(["Stand::::PlayGuitar", "SitStool::::PlayFlute"]);
  });

  it("removes exactly the requested association, keeping the rest", () => {
    const storage = fakeStorage();
    addManualScenario("Item_Lute._xmsh", "Stand::::PlayGuitar", storage);
    addManualScenario("Item_Lute._xmsh", "SitStool::::PlayFlute", storage);
    removeManualScenario("Item_Lute._xmsh", "Stand::::PlayGuitar", storage);
    expect(getManualScenarioIds("Item_Lute._xmsh", storage)).toEqual(["SitStool::::PlayFlute"]);
  });

  it("keeps different items' associations independent", () => {
    const storage = fakeStorage();
    addManualScenario("Item_Lute._xmsh", "Stand::::PlayGuitar", storage);
    addManualScenario("Item_Drum._xmsh", "Stand::::PlayDrum", storage);
    expect(getManualScenarioIds("Item_Lute._xmsh", storage)).toEqual(["Stand::::PlayGuitar"]);
    expect(getManualScenarioIds("Item_Drum._xmsh", storage)).toEqual(["Stand::::PlayDrum"]);
  });
});
