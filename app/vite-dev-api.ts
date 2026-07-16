// Real local backend for browser-preview development, until the Tauri shell can be compiled
// in this environment (see risenlab-project memory — missing MinGW binutils/MSVC linker).
// Every endpoint here shells out to the ALREADY-WORKING `risenlab` CLI (target/debug/
// risenlab.exe, built from ../src) or does plain real filesystem work — nothing here
// fabricates data. This file only runs inside `vite dev` (see configureServer below); it is
// never part of the production build and has no bearing on the real Tauri backend
// (src-tauri/), which remains the intended shipped architecture.
import type { Plugin, ViteDevServer } from "vite";
import { spawn } from "node:child_process";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import type { IncomingMessage, ServerResponse } from "node:http";

const CLI_PATH = path.resolve(__dirname, "../target/debug/risenlab.exe");

function homeDesktop(): string {
  const home = process.env.USERPROFILE ?? process.cwd();
  return path.join(home, "Desktop");
}

const PROJECT_ROOT = path.join(homeDesktop(), "RisenLab-Project");
function defaultSettings() {
  return {
    gameExe: null as string | null,
    outputDir: path.join(PROJECT_ROOT, "textures"),
    patchDir: path.join(PROJECT_ROOT, "patches"),
    reviewHtml: path.join(PROJECT_ROOT, "review.html"),
    language: "uk",
    // Real AI texture enhancement (Replicate) — the Rust CLI reads these straight from
    // settings.json; empty key = feature dormant, local Lanczos fallback.
    aiApiKey: null as string | null,
    aiModel: null as string | null,
  };
}
const SETTINGS_PATH = path.join(PROJECT_ROOT, "settings.json");

async function readJsonBody(req: IncomingMessage): Promise<any> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) chunks.push(chunk as Buffer);
  const raw = Buffer.concat(chunks).toString("utf-8");
  return raw ? JSON.parse(raw) : {};
}

function sendJson(res: ServerResponse, status: number, data: unknown) {
  const body = JSON.stringify(data);
  res.statusCode = status;
  res.setHeader("Content-Type", "application/json; charset=utf-8");
  res.end(body);
}

function runCli(args: string[]): Promise<{ stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn(CLI_PATH, args, { windowsHide: true });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d.toString()));
    child.stderr.on("data", (d) => (stderr += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) resolve({ stdout, stderr });
      else reject(new Error(stderr.trim() || `risenlab.exe exited with code ${code}`));
    });
  });
}

/** Runs a tiny PowerShell script that shows a native Windows Forms dialog and prints the
 * chosen path (or nothing if cancelled) — a real OS file/folder picker, since a browser tab
 * has no API for picking arbitrary local paths. */
function runPowerShellDialog(script: string): Promise<string | null> {
  return new Promise((resolve, reject) => {
    const child = spawn("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
      windowsHide: true,
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d.toString()));
    child.stderr.on("data", (d) => (stderr += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => {
      if (code !== 0) return reject(new Error(stderr.trim() || `powershell exited with code ${code}`));
      const trimmed = stdout.trim();
      resolve(trimmed.length > 0 ? trimmed : null);
    });
  });
}

async function loadSettings(): Promise<ReturnType<typeof defaultSettings>> {
  try {
    const raw = await fs.readFile(SETTINGS_PATH, "utf-8");
    return { ...defaultSettings(), ...JSON.parse(raw) };
  } catch {
    return defaultSettings();
  }
}

async function saveSettings(settings: ReturnType<typeof defaultSettings>): Promise<void> {
  await fs.mkdir(path.dirname(SETTINGS_PATH), { recursive: true });
  await fs.writeFile(SETTINGS_PATH, JSON.stringify(settings, null, 2), "utf-8");
}

// Real mesh count for the Dashboard's "Моделі доступні" tile (1666 real meshes as of the
// current game data — was hardcoded to a stale "3, demo set" from before the real Models
// pipeline shipped). `list-meshes` only reads archive directory listings (no per-mesh
// conversion) but still takes ~8s against the real game on this machine — too slow to redo on
// every dashboard load/poll, so it's cached per `gameExe` path for the life of this dev server.
let modelsAvailableCache: { gameExe: string; count: number } | null = null;

async function modelsAvailableCached(gameExe: string | null): Promise<number> {
  if (!gameExe) return 0;
  if (modelsAvailableCache?.gameExe === gameExe) return modelsAvailableCache.count;
  try {
    const { stdout } = await runCli(["list-meshes", gameExe]);
    const count = (JSON.parse(stdout) as unknown[]).length;
    modelsAvailableCache = { gameExe, count };
    return count;
  } catch {
    return 0;
  }
}

async function loadReviewStatus(outputDir: string): Promise<Record<string, string>> {
  try {
    const raw = await fs.readFile(path.join(outputDir, "review_status.json"), "utf-8");
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

async function saveReviewStatus(outputDir: string, status: Record<string, string>): Promise<void> {
  await fs.writeFile(path.join(outputDir, "review_status.json"), JSON.stringify(status, null, 2), "utf-8");
}

async function copyDirRecursive(src: string, dest: string): Promise<void> {
  await fs.mkdir(dest, { recursive: true });
  const entries = await fs.readdir(src, { withFileTypes: true });
  for (const entry of entries) {
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);
    if (entry.isDirectory()) await copyDirRecursive(srcPath, destPath);
    else await fs.copyFile(srcPath, destPath);
  }
}

function timestamp(): string {
  return new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
}

async function dirSizeBytes(dir: string): Promise<number> {
  let total = 0;
  let entries;
  try {
    entries = await fs.readdir(dir, { withFileTypes: true });
  } catch {
    return 0;
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) total += await dirSizeBytes(full);
    else total += (await fs.stat(full)).size;
  }
  return total;
}

async function listEditedPngs(outputDir: string): Promise<string[]> {
  const editedRoot = path.join(outputDir, "edited");
  const out: string[] = [];
  async function walk(dir: string) {
    let entries;
    try {
      entries = await fs.readdir(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) await walk(full);
      else if (entry.name.endsWith(".png")) out.push(path.relative(editedRoot, full).replace(/\\/g, "/"));
    }
  }
  await walk(editedRoot);
  return out.sort();
}

export function risenlabDevApi(): Plugin {
  return {
    name: "risenlab-dev-api",
    configureServer(server: ViteDevServer) {
      server.middlewares.use(async (req, res, next) => {
        if (!req.url?.startsWith("/api/")) return next();
        const url = new URL(req.url, "http://localhost");
        try {
          if (url.pathname === "/api/settings" && req.method === "GET") {
            return sendJson(res, 200, await loadSettings());
          }
          if (url.pathname === "/api/settings" && req.method === "POST") {
            const body = await readJsonBody(req);
            await saveSettings(body);
            return sendJson(res, 200, { ok: true });
          }
          if (url.pathname === "/api/pick-file" && req.method === "POST") {
            const p = await runPowerShellDialog(
              `Add-Type -AssemblyName System.Windows.Forms | Out-Null
               $f = New-Object System.Windows.Forms.OpenFileDialog
               $f.Filter = 'Risen.exe or shortcut (*.exe;*.lnk)|*.exe;*.lnk|All files|*.*'
               if ($f.ShowDialog() -eq 'OK') { Write-Output $f.FileName }`,
            );
            return sendJson(res, 200, { path: p });
          }
          if (url.pathname === "/api/pick-folder" && req.method === "POST") {
            const p = await runPowerShellDialog(
              `Add-Type -AssemblyName System.Windows.Forms | Out-Null
               $f = New-Object System.Windows.Forms.FolderBrowserDialog
               if ($f.ShowDialog() -eq 'OK') { Write-Output $f.SelectedPath }`,
            );
            return sendJson(res, 200, { path: p });
          }
          if (url.pathname === "/api/check-game" && req.method === "POST") {
            const settings = await loadSettings();
            if (!settings.gameExe) return sendJson(res, 400, { error: "No game path set" });
            const { stdout: discoverOut } = await runCli(["discover", settings.gameExe]);
            const archiveLines = discoverOut
              .split("\n")
              .filter((l) => l.trim().startsWith("["))
              .map((l) => l.replace(/^\s*\[[^\]]+\]\s*/, "").trim());
            let totalBytes = 0;
            for (const p of archiveLines) {
              try {
                totalBytes += (await fs.stat(p)).size;
              } catch {
                /* archive listed but unreadable — skip its size, don't fail the whole check */
              }
            }
            await runCli(["extract-textures", settings.gameExe, settings.outputDir]);
            const entries = JSON.parse((await runCli(["list-library", settings.outputDir])).stdout);
            return sendJson(res, 200, {
              root: path.dirname(settings.gameExe),
              archiveCount: archiveLines.length,
              totalBytes,
              texturesExtracted: entries.length,
            });
          }
          if (url.pathname === "/api/stats" && req.method === "GET") {
            const settings = await loadSettings();
            const modelsAvailable = await modelsAvailableCached(settings.gameExe);
            let textureTotal = 0;
            try {
              const entries = JSON.parse((await runCli(["list-library", settings.outputDir])).stdout);
              textureTotal = entries.length;
            } catch {
              /* nothing extracted yet — 0 is the honest answer, not an error */
            }
            const status = await loadReviewStatus(settings.outputDir);
            const textureProcessed = Object.keys(status).length;
            let gameArchiveTotalBytes: number | null = null;
            let archiveCount: number | null = null;
            if (settings.gameExe) {
              try {
                const { stdout: discoverOut } = await runCli(["discover", settings.gameExe]);
                const archiveLines = discoverOut
                  .split("\n")
                  .filter((l) => l.trim().startsWith("["))
                  .map((l) => l.replace(/^\s*\[[^\]]+\]\s*/, "").trim());
                archiveCount = archiveLines.length;
                gameArchiveTotalBytes = 0;
                for (const p of archiveLines) {
                  try {
                    gameArchiveTotalBytes += (await fs.stat(p)).size;
                  } catch {
                    /* skip unreadable archive path */
                  }
                }
              } catch {
                /* game path set but not resolvable right now — leave archive stats null */
              }
            }
            const outputDirSizeBytes = await dirSizeBytes(settings.outputDir);
            return sendJson(res, 200, {
              textureTotal,
              textureProcessed,
              archiveCount,
              gameArchiveTotalBytes,
              outputDirSizeBytes,
              modelsAvailable,
              appVersion: "0.1.0",
            });
          }
          if (url.pathname === "/api/list-library" && req.method === "GET") {
            const outputDir = url.searchParams.get("outputDir")!;
            const { stdout } = await runCli(["list-library", outputDir]);
            return sendJson(res, 200, JSON.parse(stdout));
          }
          if (url.pathname === "/api/list-meshes" && req.method === "GET") {
            const settings = await loadSettings();
            if (!settings.gameExe) return sendJson(res, 400, { error: "No game path set" });
            const { stdout } = await runCli(["list-meshes", settings.gameExe]);
            return sendJson(res, 200, JSON.parse(stdout));
          }
          if (url.pathname === "/api/mesh-obj" && req.method === "GET") {
            const archivePath = url.searchParams.get("archivePath")!;
            const entryPath = url.searchParams.get("entryPath")!;
            const settings = await loadSettings();
            const meshCacheDir = path.join(path.dirname(settings.outputDir), "meshes");
            const { stdout } = await runCli(["mesh-to-obj-from-archive", archivePath, entryPath, meshCacheDir]);
            const objPath = JSON.parse(stdout) as string;
            // Best-effort: makes the exported .obj self-sufficient in any real 3D tool (real
            // map_Kd/map_bump paths, not just this app's own name-matching) — see
            // batch::embed_real_texture_paths. Never blocks the response on failure (e.g. no
            // texture library extracted yet); this app's own view doesn't depend on it.
            await runCli(["embed-real-texture-paths", objPath, settings.outputDir]).catch(() => {});
            const data = await fs.readFile(objPath, "utf-8");
            res.statusCode = 200;
            res.setHeader("Content-Type", "text/plain; charset=utf-8");
            res.setHeader("Cache-Control", "no-cache");
            return res.end(data);
          }
          if (url.pathname === "/api/list-actors" && req.method === "GET") {
            const settings = await loadSettings();
            if (!settings.gameExe) return sendJson(res, 400, { error: "No game path set" });
            const { stdout } = await runCli(["list-actors", settings.gameExe]);
            return sendJson(res, 200, JSON.parse(stdout));
          }
          if (url.pathname === "/api/actor-obj" && req.method === "GET") {
            const archivePath = url.searchParams.get("archivePath")!;
            const entryPath = url.searchParams.get("entryPath")!;
            const settings = await loadSettings();
            const actorCacheDir = path.join(path.dirname(settings.outputDir), "actors");
            const { stdout } = await runCli(["actor-to-obj-from-archive", archivePath, entryPath, actorCacheDir]);
            const objPath = JSON.parse(stdout) as string;
            // See the matching comment in /api/mesh-obj.
            await runCli(["embed-real-texture-paths", objPath, settings.outputDir]).catch(() => {});
            const data = await fs.readFile(objPath, "utf-8");
            res.statusCode = 200;
            res.setHeader("Content-Type", "text/plain; charset=utf-8");
            res.setHeader("Cache-Control", "no-cache");
            return res.end(data);
          }
          if (url.pathname === "/api/list-motions" && req.method === "GET") {
            const settings = await loadSettings();
            if (!settings.gameExe) return sendJson(res, 400, { error: "No game path set" });
            const { stdout } = await runCli(["list-motions", settings.gameExe]);
            return sendJson(res, 200, JSON.parse(stdout));
          }
          if (url.pathname === "/api/mesh-texture-refs" && req.method === "GET") {
            const archivePath = url.searchParams.get("archivePath")!;
            const entryPath = url.searchParams.get("entryPath")!;
            const kind = url.searchParams.get("kind") === "actor" ? "actor-to-obj-from-archive" : "mesh-to-obj-from-archive";
            const settings = await loadSettings();
            const cacheDir = path.join(path.dirname(settings.outputDir), kind === "actor-to-obj-from-archive" ? "actors" : "meshes");
            const { stdout: objStdout } = await runCli([kind, archivePath, entryPath, cacheDir]);
            const objPath = JSON.parse(objStdout) as string;
            const { stdout: refsStdout } = await runCli(["mesh-texture-refs", objPath]);
            return sendJson(res, 200, JSON.parse(refsStdout));
          }
          if (url.pathname === "/api/actor-skeleton" && req.method === "GET") {
            const archivePath = url.searchParams.get("archivePath")!;
            const entryPath = url.searchParams.get("entryPath")!;
            const { stdout } = await runCli(["actor-skeleton", archivePath, entryPath]);
            return sendJson(res, 200, JSON.parse(stdout));
          }
          if (url.pathname === "/api/motion-tracks" && req.method === "GET") {
            const archivePath = url.searchParams.get("archivePath")!;
            const entryPath = url.searchParams.get("entryPath")!;
            const boneNamesJson = url.searchParams.get("boneNames")!;
            const { stdout } = await runCli(["motion-tracks", archivePath, entryPath, boneNamesJson]);
            return sendJson(res, 200, JSON.parse(stdout));
          }
          if (url.pathname === "/api/actor-skinned-mesh" && req.method === "GET") {
            const archivePath = url.searchParams.get("archivePath")!;
            const entryPath = url.searchParams.get("entryPath")!;
            const { stdout } = await runCli(["actor-skinned-mesh", archivePath, entryPath]);
            return sendJson(res, 200, JSON.parse(stdout));
          }
          if (url.pathname === "/api/texture" && req.method === "GET") {
            const outputDir = url.searchParams.get("outputDir")!;
            const pngRel = url.searchParams.get("pngRel")!;
            const edited = url.searchParams.get("edited") === "1";
            const filePath = path.join(outputDir, edited ? "edited" : "", pngRel);
            const data = await fs.readFile(filePath);
            res.statusCode = 200;
            res.setHeader("Content-Type", "image/png");
            res.setHeader("Cache-Control", "no-cache");
            return res.end(data);
          }
          if (url.pathname === "/api/texture-meta" && req.method === "GET") {
            const archivePath = url.searchParams.get("archivePath")!;
            const entryPath = url.searchParams.get("entryPath")!;
            const { stdout } = await runCli(["texture-meta", archivePath, entryPath]);
            return sendJson(res, 200, JSON.parse(stdout));
          }
          if (url.pathname === "/api/regenerate" && req.method === "POST") {
            const { outputDir, pngRel, scale } = await readJsonBody(req);
            await runCli(["regenerate", outputDir, pngRel, String(scale ?? 2)]);
            return sendJson(res, 200, { ok: true });
          }
          if (url.pathname === "/api/review-queue" && req.method === "GET") {
            const outputDir = url.searchParams.get("outputDir")!;
            const [edited, status] = await Promise.all([listEditedPngs(outputDir), loadReviewStatus(outputDir)]);
            return sendJson(
              res,
              200,
              edited.map((pngRel) => ({ pngRel, status: status[pngRel] ?? "pending" })),
            );
          }
          if (url.pathname === "/api/review-status" && req.method === "POST") {
            const { outputDir, pngRel, status } = await readJsonBody(req);
            const map = await loadReviewStatus(outputDir);
            if (status === "rejected") {
              delete map[pngRel];
              await fs.unlink(path.join(outputDir, "edited", pngRel)).catch(() => {});
            } else {
              map[pngRel] = status;
            }
            await saveReviewStatus(outputDir, map);
            return sendJson(res, 200, { ok: true });
          }
          if (url.pathname === "/api/build-patches" && req.method === "POST") {
            const settings = await loadSettings();
            const status = await loadReviewStatus(settings.outputDir);
            const approved = Object.entries(status)
              .filter(([, s]) => s === "approved")
              .map(([pngRel]) => pngRel);
            const stageDir = path.join(settings.outputDir, "_approved_stage");
            await fs.rm(stageDir, { recursive: true, force: true });
            for (const pngRel of approved) {
              const src = path.join(settings.outputDir, "edited", pngRel);
              const dest = path.join(stageDir, pngRel);
              try {
                await fs.mkdir(path.dirname(dest), { recursive: true });
                await fs.copyFile(src, dest);
              } catch {
                /* approved entry with no edited file on disk (e.g. manual removal) — skip it */
              }
            }
            const manifest = path.join(settings.outputDir, "manifest.tsv");
            const { stdout } = await runCli(["apply-textures", manifest, stageDir, settings.patchDir]);
            await fs.rm(stageDir, { recursive: true, force: true });
            const written = [...stdout.matchAll(/^\s*(.+\.p\d+)\s*$/gm)].map((m) => m[1].trim());
            return sendJson(res, 200, written);
          }
          if (url.pathname === "/api/backup" && req.method === "POST") {
            // Everything lives under one project root (outputDir/patchDir/reviewHtml are all
            // subpaths of it by default) — back up that whole root to a timestamped sibling
            // folder so work can be resumed/rolled back later.
            const settings = await loadSettings();
            const projectRoot = path.dirname(settings.outputDir);
            const backupsRoot = path.join(path.dirname(projectRoot), "RisenLab-Backups");
            const dest = path.join(backupsRoot, `backup-${timestamp()}`);
            await copyDirRecursive(projectRoot, dest);
            return sendJson(res, 200, { path: dest });
          }
          return sendJson(res, 404, { error: "not found" });
        } catch (err) {
          return sendJson(res, 500, { error: err instanceof Error ? err.message : String(err) });
        }
      });
    },
  };
}
