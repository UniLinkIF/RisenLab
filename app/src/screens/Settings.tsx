import { useEffect, useState } from "react";
import type { Lang } from "../lib/i18n";
import { t } from "../lib/i18n";
import type { AppSettings, GameCheckResult } from "../lib/types";
import { backupProject, buildPatches, checkGame, getSettings, installPatches, pickFolder, pickGamePath, saveSettings, uninstallPatches } from "../lib/api";
import { formatBytes } from "../lib/library";

interface Props {
  lang: Lang;
  onLangChange: (l: Lang) => void;
  onSettingsSaved: (settings: AppSettings) => void;
}

export default function Settings({ lang, onLangChange, onSettingsSaved }: Props) {
  const s = t(lang);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<GameCheckResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [patchMessage, setPatchMessage] = useState<string | null>(null);
  const [backupMessage, setBackupMessage] = useState<string | null>(null);

  useEffect(() => {
    getSettings().then(setSettings);
  }, []);

  async function persist(next: AppSettings) {
    setSettings(next);
    await saveSettings(next);
    onSettingsSaved(next);
  }

  async function browse() {
    if (!settings) return;
    const path = await pickGamePath();
    if (path) await persist({ ...settings, gameExe: path });
  }

  async function check() {
    setChecking(true);
    setError(null);
    try {
      const r = await checkGame();
      setResult(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setChecking(false);
    }
  }

  async function onBuildPatch() {
    setPatchMessage(null);
    try {
      const written = await buildPatches();
      setPatchMessage(written.length ? written.join(", ") : lang === "uk" ? "Немає прийнятих текстур" : "No approved textures");
    } catch (e) {
      setPatchMessage(String(e));
    }
  }

  async function onInstallPatches() {
    setPatchMessage(null);
    try {
      const installed = await installPatches();
      setPatchMessage(
        installed.length
          ? (lang === "uk" ? "Встановлено в гру: " : "Installed into the game: ") + installed.join(", ")
          : lang === "uk"
            ? "Немає зібраних патчів (спершу «Зібрати патч»)"
            : "No built patches (run “Build patch” first)",
      );
    } catch (e) {
      setPatchMessage(String(e));
    }
  }

  async function onUninstallPatches() {
    setPatchMessage(null);
    try {
      const removed = await uninstallPatches();
      setPatchMessage(
        removed.length
          ? (lang === "uk" ? "Прибрано з гри: " : "Removed from the game: ") + removed.join(", ")
          : lang === "uk"
            ? "У грі немає наших патчів"
            : "No patches of ours in the game",
      );
    } catch (e) {
      setPatchMessage(String(e));
    }
  }

  async function onBackup() {
    setBackupMessage(lang === "uk" ? "Створення копії…" : "Backing up…");
    try {
      const dest = await backupProject();
      setBackupMessage(dest);
    } catch (e) {
      setBackupMessage(String(e));
    }
  }

  if (!settings) return null;

  async function browseFolder(setValue: (v: string) => Promise<void>) {
    const path = await pickFolder();
    if (path) await setValue(path);
  }

  const pathRows: Array<[string, string, (v: string) => Promise<void>]> = [
    [
      lang === "uk" ? "Текстури" : "Textures",
      settings.outputDir,
      (v) => persist({ ...settings, outputDir: v }),
    ],
    [
      lang === "uk" ? "Патчі" : "Patches",
      settings.patchDir,
      (v) => persist({ ...settings, patchDir: v }),
    ],
    [
      lang === "uk" ? "Огляд (HTML)" : "Review (HTML)",
      settings.reviewHtml,
      (v) => persist({ ...settings, reviewHtml: v }),
    ],
  ];

  return (
    <div style={{ flex: 1, overflow: "auto", padding: "30px 0", display: "flex", justifyContent: "center" }}>
      <div style={{ width: 640, display: "flex", flexDirection: "column", gap: 20 }}>
        <div style={{ font: "700 20px system-ui" }}>{s.settingsTitle}</div>

        <div style={{ background: "var(--bg1)", border: "1px solid var(--border)", borderRadius: 14, padding: 20 }}>
          <div style={{ font: "600 11px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: "var(--text-faint)", marginBottom: 12 }}>
            {s.gameSection}
          </div>
          <div style={{ font: "500 12.5px system-ui", color: "var(--text-dim)", marginBottom: 6 }}>{s.gamePath}</div>
          <div style={{ display: "flex", gap: 8 }}>
            <div style={{ flex: 1, background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 9, padding: "10px 14px", font: "500 12.5px ui-monospace, Menlo, monospace", color: "var(--text)" }}>
              {settings.gameExe ?? "—"}
            </div>
            <button onClick={browse} style={{ padding: "10px 18px", borderRadius: 9, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 12.5px system-ui", color: "var(--text)", whiteSpace: "nowrap" }}>
              {s.browse}
            </button>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 14 }}>
            <button
              onClick={check}
              disabled={checking || !settings.gameExe}
              style={{ padding: "10px 18px", borderRadius: 9, background: "var(--accent)", border: "none", font: "700 12.5px system-ui", color: "#fff", opacity: checking || !settings.gameExe ? 0.6 : 1 }}
            >
              {checking ? s.loading : s.check}
            </button>
            {result ? (
              <div style={{ display: "flex", alignItems: "center", gap: 7, font: "500 12.5px system-ui", color: "var(--text-dim)" }}>
                <div style={{ width: 7, height: 7, borderRadius: "50%", background: "var(--green)" }} />
                {s.found}
              </div>
            ) : null}
          </div>
          {error ? <div style={{ color: "var(--red)", marginTop: 10, fontSize: 12 }}>{error}</div> : null}
          {result ? (
            <div style={{ marginTop: 14, display: "flex", gap: 24 }}>
              <div>
                <div style={{ font: "600 20px system-ui", color: "var(--text)" }}>{result.texturesExtracted}</div>
                <div style={{ font: "500 11px system-ui", color: "var(--text-faint)" }}>{s.textures}</div>
              </div>
              <div>
                <div style={{ font: "600 20px system-ui", color: "var(--text)" }}>{result.archiveCount}</div>
                <div style={{ font: "500 11px system-ui", color: "var(--text-faint)" }}>{s.archives}</div>
              </div>
              <div>
                <div style={{ font: "600 20px system-ui", color: "var(--text)" }}>{formatBytes(result.totalBytes)}</div>
                <div style={{ font: "500 11px system-ui", color: "var(--text-faint)" }}>images.pak</div>
              </div>
            </div>
          ) : null}
        </div>

        <div style={{ background: "var(--bg1)", border: "1px solid var(--border)", borderRadius: 14, padding: 20 }}>
          <div style={{ font: "600 11px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: "var(--text-faint)", marginBottom: 12 }}>
            {s.outputSection}
          </div>
          {pathRows.map(([label, value, setValue]) => (
            <div key={label} style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
              <div style={{ width: 130, flexShrink: 0, font: "500 12.5px system-ui", color: "var(--text-dim)" }}>{label}</div>
              <input
                value={value}
                onChange={(e) => setValue(e.target.value)}
                style={{ flex: 1, background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: "9px 12px", font: "500 12px ui-monospace, Menlo, monospace", color: "var(--text)" }}
              />
              <button
                onClick={() => browseFolder(setValue)}
                style={{ padding: "9px 14px", borderRadius: 8, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 12px system-ui", color: "var(--text)", whiteSpace: "nowrap" }}
              >
                {s.browse}
              </button>
            </div>
          ))}
        </div>

        <div style={{ background: "var(--bg1)", border: "1px solid var(--border)", borderRadius: 14, padding: 20 }}>
          <div style={{ font: "600 11px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: "var(--text-faint)", marginBottom: 12 }}>
            {lang === "uk" ? "ШІ — покращення текстур" : "AI — texture enhancement"}
          </div>
          <div style={{ font: "500 12px system-ui", color: "var(--text-faint)", marginBottom: 12, lineHeight: 1.5 }}>
            {lang === "uk"
              ? "Встав API-ключ Replicate (replicate.com → Account → API tokens) — і кнопки «Покращити текстури» почнуть використовувати справжній ШІ замість локального збільшення. Без ключа все працює як зараз (Lanczos). Normal-мапи ШІ не чіпає ніколи — вони лишаються на локальному шляху."
              : "Paste a Replicate API token (replicate.com → Account → API tokens) and the “Enhance textures” buttons switch to real AI instead of the local upscale. Without a key everything keeps working as today (Lanczos). Normal maps never go through AI — they stay on the local path."}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
            <div style={{ width: 130, flexShrink: 0, font: "500 12.5px system-ui", color: "var(--text-dim)" }}>
              {lang === "uk" ? "API-ключ" : "API key"}
            </div>
            <input
              type="password"
              value={settings.aiApiKey ?? ""}
              placeholder="r8_…"
              autoComplete="off"
              onChange={(e) => persist({ ...settings, aiApiKey: e.target.value.trim() || null })}
              style={{ flex: 1, background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: "9px 12px", font: "500 12px ui-monospace, Menlo, monospace", color: "var(--text)" }}
            />
            <div
              style={{
                width: 9,
                height: 9,
                borderRadius: "50%",
                flexShrink: 0,
                background: settings.aiApiKey ? "var(--green)" : "var(--border)",
              }}
              title={
                settings.aiApiKey
                  ? lang === "uk"
                    ? "ШІ активний"
                    : "AI active"
                  : lang === "uk"
                    ? "Без ключа — локальне покращення"
                    : "No key — local enhancement"
              }
            />
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <div style={{ width: 130, flexShrink: 0, font: "500 12.5px system-ui", color: "var(--text-dim)" }}>
              {lang === "uk" ? "Модель" : "Model"}
            </div>
            <input
              value={settings.aiModel ?? ""}
              placeholder="nightmareai/real-esrgan"
              onChange={(e) => persist({ ...settings, aiModel: e.target.value.trim() || null })}
              style={{ flex: 1, background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: "9px 12px", font: "500 12px ui-monospace, Menlo, monospace", color: "var(--text)" }}
            />
          </div>
          <div style={{ font: "500 11px system-ui", color: "var(--text-faint)", marginTop: 8, lineHeight: 1.5 }}>
            {lang === "uk"
              ? "Порожньо = точне збільшення (real-esrgan, без промпту). Можна вказати будь-яку img2img-модель Replicate (owner/name) — тоді застосуються промпти за категорією текстури (шкіра/метал/камінь/тканина…)."
              : "Empty = faithful upscale (real-esrgan, no prompt). Any Replicate img2img model (owner/name) switches to category prompts (skin/metal/stone/cloth…)."}
          </div>
        </div>

        <div style={{ background: "var(--bg1)", border: "1px solid var(--border)", borderRadius: 14, padding: 20 }}>
          <div style={{ font: "600 11px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: "var(--text-faint)", marginBottom: 12 }}>
            {s.appSection}
          </div>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "6px 0" }}>
            <div style={{ font: "500 12.5px system-ui", color: "var(--text-dim)" }}>{s.language}</div>
            <div style={{ display: "flex", background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 20, padding: 3, gap: 2 }}>
              {(["uk", "en"] as Lang[]).map((l) => (
                <button
                  key={l}
                  onClick={() => onLangChange(l)}
                  style={{
                    padding: "5px 14px",
                    borderRadius: 16,
                    border: "none",
                    font: "600 11px system-ui",
                    background: lang === l ? "var(--accent)" : "transparent",
                    color: lang === l ? "#fff" : "var(--text-faint)",
                  }}
                >
                  {l === "uk" ? "Українська" : "English"}
                </button>
              ))}
            </div>
          </div>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 0 0" }}>
            <div style={{ font: "500 12.5px system-ui", color: "var(--text-dim)" }}>{s.buildPatch}</div>
            <div style={{ display: "flex", gap: 8 }}>
              <button onClick={onBuildPatch} style={{ padding: "8px 16px", borderRadius: 9, background: "var(--accent)", border: "none", font: "600 12px system-ui", color: "#fff" }}>
                {s.buildPatch}
              </button>
              <button
                onClick={onInstallPatches}
                disabled={!settings.gameExe}
                title={
                  lang === "uk"
                    ? "Скопіювати всі зібрані .pXX у теку гри (data/compiled, data/common). Відкат — «Прибрати з гри»."
                    : "Copy all built .pXX volumes into the game's data folders. Revert with “Remove from game”."
                }
                style={{ padding: "8px 16px", borderRadius: 9, background: "var(--bg2)", border: "1px solid var(--accent)", font: "600 12px system-ui", color: "var(--text)", opacity: settings.gameExe ? 1 : 0.5 }}
              >
                {lang === "uk" ? "🎮 Встановити в гру" : "🎮 Install into game"}
              </button>
              <button
                onClick={onUninstallPatches}
                disabled={!settings.gameExe}
                title={
                  lang === "uk"
                    ? "Видалити з теки гри лише файли, що є серед зібраних патчів — більше нічого не чіпається."
                    : "Delete from the game folder only files that also exist among the built patches — nothing else is touched."
                }
                style={{ padding: "8px 16px", borderRadius: 9, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 12px system-ui", color: "var(--text-dim)", opacity: settings.gameExe ? 1 : 0.5 }}
              >
                {lang === "uk" ? "Прибрати з гри" : "Remove from game"}
              </button>
            </div>
          </div>
          {patchMessage ? <div style={{ marginTop: 8, fontSize: 12, color: "var(--text-faint)", wordBreak: "break-all" }}>{patchMessage}</div> : null}
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 0 0" }}>
            <div style={{ font: "500 12.5px system-ui", color: "var(--text-dim)" }}>
              {lang === "uk" ? "Резервна копія проєкту" : "Project backup"}
            </div>
            <button onClick={onBackup} style={{ padding: "8px 16px", borderRadius: 9, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 12px system-ui", color: "var(--text)" }}>
              {lang === "uk" ? "Зробити копію" : "Back up now"}
            </button>
          </div>
          {backupMessage ? <div style={{ marginTop: 8, fontSize: 12, color: "var(--text-faint)", wordBreak: "break-all" }}>{backupMessage}</div> : null}
        </div>
      </div>
    </div>
  );
}
