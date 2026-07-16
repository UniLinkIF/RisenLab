import { describe, expect, it } from "vitest";
import { queueCount, STR, t } from "./i18n";

describe("t", () => {
  it("returns the uk dictionary for uk", () => {
    expect(t("uk")).toBe(STR.uk);
    expect(t("uk").settingsTitle).toBe("Налаштування");
  });

  it("returns the en dictionary for en", () => {
    expect(t("en")).toBe(STR.en);
    expect(t("en").settingsTitle).toBe("Settings");
  });
});

describe("queueCount", () => {
  it("formats the review queue count in Ukrainian", () => {
    expect(queueCount(12, "uk")).toBe("12 текстур в черзі перевірки");
  });

  it("formats the review queue count in English", () => {
    expect(queueCount(12, "en")).toBe("12 textures in review queue");
  });
});
