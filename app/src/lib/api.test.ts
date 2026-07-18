// api.ts is the one file both real backends (Tauri commands / the dev-only HTTP bridge) have
// to agree with, param-name for param-name — nothing else catches a mismatch between the two
// (see risenlab-full-audit: this was the single biggest untested file). vitest's `environment:
// "node"` (vite.config.ts) means `window` is undefined by default, which conveniently makes
// `isTauri()` false out of the box — the dev-api/fetch branch needs no setup, and the Tauri
// branch is exercised by stubbing `window.__TAURI_INTERNALS__` per test.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function jsonResponse(body: unknown): Response {
  return { ok: true, json: async () => body } as Response;
}

function useTauri() {
  (globalThis as { window?: unknown }).window = { __TAURI_INTERNALS__: {} };
}

describe("api.ts dual-backend param shapes", () => {
  let fetchMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    invokeMock.mockReset();
    fetchMock = vi.fn(async (url: string) => {
      if (url.startsWith("/api/settings")) return jsonResponse({ outputDir: "C:/out", patchDir: "C:/patch", reviewHtml: "C:/r.html", language: "uk", aiProvider: null, aiApiKey: null, aiModel: null });
      return jsonResponse([]);
    });
    vi.stubGlobal("fetch", fetchMock);
    delete (globalThis as { window?: unknown }).window;
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    delete (globalThis as { window?: unknown }).window;
  });

  describe("motionTracks", () => {
    it("dev-api: encodes every MotionStyle field into the query string", async () => {
      const { motionTracks } = await import("./api");
      await motionTracks("archive.pak", "/entry.xmot", ["Bone_ROOT"], { smooth: 0.5, expressiveness: 0.25, secondary: 0.4, sharpness: 0.8, doubleRate: true });
      const url = fetchMock.mock.calls[0][0] as string;
      expect(url).toContain("/api/motion-tracks?");
      expect(url).toContain("smooth=0.5");
      expect(url).toContain("expressiveness=0.25");
      expect(url).toContain("secondary=0.4");
      expect(url).toContain("sharpness=0.8");
      expect(url).toContain("doubleRate=true");
    });

    it("dev-api: defaults every MotionStyle field to 0/false when omitted", async () => {
      const { motionTracks } = await import("./api");
      await motionTracks("archive.pak", "/entry.xmot", ["Bone_ROOT"]);
      const url = fetchMock.mock.calls[0][0] as string;
      expect(url).toContain("smooth=0");
      expect(url).toContain("expressiveness=0");
      expect(url).toContain("secondary=0");
      expect(url).toContain("sharpness=0");
      expect(url).toContain("doubleRate=false");
    });

    it("Tauri: invokes motion_tracks with the exact same field names as the query string uses", async () => {
      useTauri();
      const { motionTracks } = await import("./api");
      invokeMock.mockResolvedValue([]);
      await motionTracks("archive.pak", "/entry.xmot", ["Bone_ROOT"], { smooth: 0.5, expressiveness: 0.25, secondary: 0.4, sharpness: 0.8, doubleRate: true });
      expect(invokeMock).toHaveBeenCalledWith("motion_tracks", {
        archivePath: "archive.pak",
        entryPath: "/entry.xmot",
        boneNames: ["Bone_ROOT"],
        smooth: 0.5,
        expressiveness: 0.25,
        secondary: 0.4,
        sharpness: 0.8,
        doubleRate: true,
      });
    });
  });

  describe("exportMotionPatch / exportMotionPatchBatch", () => {
    it("dev-api: POST body carries the full MotionStyle, defaulted", async () => {
      const { exportMotionPatch } = await import("./api");
      fetchMock.mockResolvedValueOnce(jsonResponse({ patch: "out.p01" }));
      await exportMotionPatch("archive.pak", "/entry.xmot", ["Bone_ROOT"], { smooth: 0.6 });
      const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
      expect(url).toBe("/api/export-motion-patch");
      const body = JSON.parse(init.body as string);
      expect(body).toEqual({
        archivePath: "archive.pak",
        entryPath: "/entry.xmot",
        boneNames: ["Bone_ROOT"],
        smooth: 0.6,
        expressiveness: 0,
        secondary: 0,
        sharpness: 0,
      });
    });

    it("Tauri: invokes export_motion_patch with matching field names (no doubleRate — not exportable)", async () => {
      useTauri();
      const { exportMotionPatch } = await import("./api");
      invokeMock.mockResolvedValue("out.p01");
      await exportMotionPatch("archive.pak", "/entry.xmot", ["Bone_ROOT"], { smooth: 0.6, expressiveness: 0.2 });
      expect(invokeMock).toHaveBeenCalledWith("export_motion_patch", {
        archivePath: "archive.pak",
        entryPath: "/entry.xmot",
        boneNames: ["Bone_ROOT"],
        smooth: 0.6,
        expressiveness: 0.2,
        secondary: 0,
        sharpness: 0,
      });
    });

    it("dev-api batch: POST body carries entryPaths (plural) and the full style", async () => {
      const { exportMotionPatchBatch } = await import("./api");
      fetchMock.mockResolvedValueOnce(jsonResponse({ patch: "out.p01", failed: [] }));
      await exportMotionPatchBatch("archive.pak", ["/a.xmot", "/b.xmot"], ["Bone_ROOT"], { sharpness: 0.5 });
      const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
      expect(url).toBe("/api/export-motion-patch-batch");
      const body = JSON.parse(init.body as string);
      expect(body.entryPaths).toEqual(["/a.xmot", "/b.xmot"]);
      expect(body.sharpness).toBe(0.5);
    });
  });

  describe("getSettings", () => {
    it("dev-api: GETs /api/settings and returns it as-is", async () => {
      const { getSettings } = await import("./api");
      const s = await getSettings();
      expect(fetchMock).toHaveBeenCalledWith("/api/settings", undefined);
      expect(s.outputDir).toBe("C:/out");
    });

    it("Tauri: invokes get_settings with no args", async () => {
      useTauri();
      const { getSettings } = await import("./api");
      invokeMock.mockResolvedValue({ outputDir: "C:/out", patchDir: "C:/p", reviewHtml: "C:/r", language: "uk", aiProvider: null, aiApiKey: null, aiModel: null });
      await getSettings();
      expect(invokeMock).toHaveBeenCalledWith("get_settings", undefined);
    });
  });

  describe("regenerateTexture", () => {
    it("dev-api: POSTs outputDir (looked up from settings) + pngRel + scale", async () => {
      const { regenerateTexture } = await import("./api");
      fetchMock.mockImplementation(async (url: string) => {
        if (url.startsWith("/api/settings")) return jsonResponse({ outputDir: "C:/out", patchDir: "C:/p", reviewHtml: "C:/r", language: "uk", aiProvider: null, aiApiKey: null, aiModel: null });
        return jsonResponse({});
      });
      await regenerateTexture("Special/Axe_Diffuse_01.png", 2);
      const postCall = fetchMock.mock.calls.find(([url]) => url === "/api/regenerate");
      expect(postCall).toBeTruthy();
      const body = JSON.parse((postCall![1] as RequestInit).body as string);
      expect(body).toEqual({ outputDir: "C:/out", pngRel: "Special/Axe_Diffuse_01.png", scale: 2 });
    });

    it("Tauri: invokes regenerate_texture with pngRel + scale (no outputDir — Tauri already knows it)", async () => {
      useTauri();
      const { regenerateTexture } = await import("./api");
      invokeMock.mockResolvedValue(undefined);
      await regenerateTexture("Special/Axe_Diffuse_01.png", 2);
      expect(invokeMock).toHaveBeenCalledWith("regenerate_texture", { pngRel: "Special/Axe_Diffuse_01.png", scale: 2 });
    });
  });
});
