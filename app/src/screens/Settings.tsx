import { useEffect, useRef, useState } from "react";
import type { Lang } from "../lib/i18n";
import { t } from "../lib/i18n";
import type { AppSettings, GameCheckResult, RemoteStatus } from "../lib/types";
import { backupProject, buildPatches, checkGame, getRemoteStatus, getSettings, installPatches, isTauri, pickFolder, pickGamePath, saveSettings, startRemoteAccess, stopRemoteAccess, uninstallPatches } from "../lib/api";
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
  // Manual build/install split (advanced) is collapsed by default — owner: "не понятно, яка
  // різниця між текстури в гру і встановити в гру" (2026-07-20). The primary flow is one
  // button ("🚀 Текстури в гру"); the two-step version underneath only matters if you want to
  // build without installing yet (e.g. to peek at the .pXX files before they touch the game).
  const [showAdvancedPatch, setShowAdvancedPatch] = useState(false);
  const [remote, setRemote] = useState<RemoteStatus | null>(null);
  const [remoteBusy, setRemoteBusy] = useState(false);
  const remotePollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    getSettings().then(setSettings);
  }, []);

  // Remote access status (see app/src-tauri/src/remote.rs) — Tauri-only, no dev-bridge
  // equivalent. Polled while the server is running because the tunnel URL isn't known the
  // instant the server starts (cloudflared takes a couple seconds to hand one out).
  useEffect(() => {
    if (!isTauri()) return;
    getRemoteStatus()
      .then((r) => {
        setRemote(r);
        if (r.running && !r.tunnelUrl) startRemotePolling();
      })
      .catch(() => {});
    return () => {
      if (remotePollRef.current) clearInterval(remotePollRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function startRemotePolling() {
    if (remotePollRef.current) return;
    remotePollRef.current = setInterval(async () => {
      const r = await getRemoteStatus().catch(() => null);
      if (!r) return;
      setRemote(r);
      if (!r.running || r.tunnelUrl) {
        if (remotePollRef.current) clearInterval(remotePollRef.current);
        remotePollRef.current = null;
      }
    }, 1500);
  }

  async function handleStartRemote() {
    setRemoteBusy(true);
    try {
      const r = await startRemoteAccess();
      setRemote(r);
      if (!r.tunnelUrl) startRemotePolling();
    } catch (e) {
      setRemote({ running: false, port: null, token: null, tunnelUrl: null, cloudflaredAvailable: false });
      setError(String(e));
    } finally {
      setRemoteBusy(false);
    }
  }

  async function handleStopRemote() {
    setRemoteBusy(true);
    try {
      await stopRemoteAccess();
      setRemote({ running: false, port: null, token: null, tunnelUrl: null, cloudflaredAvailable: remote?.cloudflaredAvailable ?? false });
      if (remotePollRef.current) {
        clearInterval(remotePollRef.current);
        remotePollRef.current = null;
      }
    } finally {
      setRemoteBusy(false);
    }
  }

  async function persist(next: AppSettings) {
    setSettings(next);
    await saveSettings(next);
    onSettingsSaved(next);
  }

  // Free-text inputs (API key, model, path fields) call this on every keystroke — writing
  // settings.json to disk that often is wasted work. Local state updates immediately (so the
  // input stays responsive), the actual disk write/onSettingsSaved is debounced.
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  function persistDebounced(next: AppSettings) {
    setSettings(next);
    if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    saveTimeoutRef.current = setTimeout(() => {
      saveTimeoutRef.current = null;
      saveSettings(next).then(() => onSettingsSaved(next));
    }, 400);
  }
  useEffect(() => {
    return () => {
      if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    };
  }, []);

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

  // The owner's "натиснув кнопку і текстури в грі": approved textures → minimal .pXX
  // patches → copied next to the game's own archives, one click. `installPatches` sweeps
  // EVERY .pXX sitting in the patch folder, not just texture ones — so this ALSO installs any
  // animation patches already built from the Animations tab ("💾 У патч"/"💾 Всі кліпи"), even
  // though this button only builds fresh TEXTURE patches itself. Owner: "чому тільки текстури
  // в гру? а анімації в гру де" (2026-07-20) — they were already being installed by this same
  // button, just under a texture-only-sounding label with no mention of the other source.
  async function onShipToGame() {
    setPatchMessage(lang === "uk" ? "Збираю патчі текстур…" : "Building texture patches…");
    try {
      const written = await buildPatches();
      setPatchMessage(lang === "uk" ? "Встановлюю в гру…" : "Installing into the game…");
      const installed = await installPatches();
      setPatchMessage(
        installed.length
          ? (lang === "uk" ? "🚀 У грі! Встановлено: " : "🚀 In the game! Installed: ") + installed.join(", ")
          : written.length
            ? (lang === "uk" ? "Патчі зібрано, але встановлено 0 — перевір шлях до гри" : "Patches built but 0 installed — check the game path")
            : lang === "uk"
              ? "Немає що встановлювати — прийми текстури в рев'ю АБО зберіть анімаційний патч на вкладці «Анімації»"
              : "Nothing to install — approve some textures in review, OR build an animation patch on the Animations tab",
      );
    } catch (e) {
      setPatchMessage(String(e));
    }
  }

  if (!settings) return null;

  async function browseFolder(setValue: (v: string) => void) {
    const path = await pickFolder();
    if (path) setValue(path);
  }

  const pathRows: Array<[string, string, (v: string) => void]> = [
    [
      lang === "uk" ? "Текстури" : "Textures",
      settings.outputDir,
      (v) => persistDebounced({ ...settings, outputDir: v }),
    ],
    [
      lang === "uk" ? "Патчі" : "Patches",
      settings.patchDir,
      (v) => persistDebounced({ ...settings, patchDir: v }),
    ],
    [
      lang === "uk" ? "Огляд (HTML)" : "Review (HTML)",
      settings.reviewHtml,
      (v) => persistDebounced({ ...settings, reviewHtml: v }),
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
              {lang === "uk" ? "Провайдер" : "Provider"}
            </div>
            {([
              ["replicate", "Replicate", lang === "uk" ? "Дефолт: real-esrgan та будь-яка img2img-модель, токен r8_…" : "Default: real-esrgan + any img2img model, r8_… token"],
              ["stability", "Stability AI", lang === "uk" ? "Conservative upscale (до 4x, кольори/розкладка недоторкані), токен sk-…" : "Conservative upscale (up to 4x, colors/layout preserved), sk-… token"],
            ] as [string, string, string][]).map(([id, label, hint]) => {
              const active = (settings.aiProvider ?? "replicate") === id;
              return (
                <button
                  key={id}
                  onClick={() => persist({ ...settings, aiProvider: id === "replicate" ? null : id })}
                  title={hint}
                  style={{ padding: "8px 14px", borderRadius: 8, background: active ? "var(--accent)" : "var(--bg2)", border: `1px solid ${active ? "var(--accent)" : "var(--border)"}`, font: "600 11.5px system-ui", color: active ? "#fff" : "var(--text-dim)", whiteSpace: "nowrap" }}
                >
                  {label}
                </button>
              );
            })}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
            <div style={{ width: 130, flexShrink: 0, font: "500 12.5px system-ui", color: "var(--text-dim)" }}>
              {lang === "uk" ? "API-ключ" : "API key"}
            </div>
            <input
              type="password"
              value={settings.aiApiKey ?? ""}
              placeholder={(settings.aiProvider ?? "replicate") === "stability" ? "sk-…" : "r8_…"}
              autoComplete="off"
              onChange={(e) => persistDebounced({ ...settings, aiApiKey: e.target.value.trim() || null })}
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
          {(settings.aiProvider ?? "replicate") === "replicate" ? (
          <>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <div style={{ width: 130, flexShrink: 0, font: "500 12.5px system-ui", color: "var(--text-dim)" }}>
              {lang === "uk" ? "Модель" : "Model"}
            </div>
            <input
              value={settings.aiModel ?? ""}
              placeholder="owner/model-name (будь-яка модель Replicate)"
              title={lang === "uk" ? "Впиши БУДЬ-ЯКУ модель з replicate.com у форматі owner/name — не лише пресети нижче." : "Type ANY model from replicate.com as owner/name — not just the presets below."}
              onChange={(e) => persistDebounced({ ...settings, aiModel: e.target.value.trim() || null })}
              style={{ flex: 1, background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 8, padding: "9px 12px", font: "500 12px ui-monospace, Menlo, monospace", color: "var(--text)" }}
            />
            {([
              ["philz1337x/clarity-upscaler", 0.5, false, lang === "uk" ? "Покращити" : "Enhance", lang === "uk" ? "Той самий кадр, та сама композиція — але з набагато детальнішою текстурою (шкіра/метал/тканина), справжній ремастер, не фільтр" : "The exact same picture, same composition — but with dramatically more detailed surface texture, a real remaster, not a filter"],
              ["stability-ai/sdxl", 0.85, true, lang === "uk" ? "✨ Нові текстури" : "✨ New textures", lang === "uk" ? "ШІ бере лише силует з оригіналу і МАЛЮЄ текстуру заново — справжня нова картинка, не фільтр" : "AI keeps only the original's silhouette and PAINTS the texture from scratch — a real new image, not a filter"],
            ] as [string | null, number, boolean, string, string][]).map(([model, creativity, regenerate, label, hint]) => {
              const active = (settings.aiModel ?? null) === model && Boolean(settings.aiRegenerate) === regenerate;
              return (
                <button
                  key={label}
                  onClick={() => persist({ ...settings, aiModel: model, aiCreativity: creativity, aiRegenerate: regenerate })}
                  title={hint}
                  style={{ padding: "8px 12px", borderRadius: 8, background: active ? "var(--accent)" : "var(--bg2)", border: `1px solid ${active ? "var(--accent)" : "var(--border)"}`, font: "600 11.5px system-ui", color: active ? "#fff" : "var(--text-dim)", whiteSpace: "nowrap" }}
                >
                  {label}
                </button>
              );
            })}
          </div>
          <div style={{ font: "500 11px system-ui", color: "var(--text-faint)", marginTop: 6, marginBottom: 10, lineHeight: 1.5 }}>
            {lang === "uk"
              ? "Порожньо = точне збільшення (real-esrgan, без промпту). Кнопки вище — лише швидкі пресети; поле моделі й повзунок нижче можна міняти незалежно від них, для будь-якої img2img-моделі Replicate."
              : "Empty = faithful upscale (real-esrgan, no prompt). The buttons above are just quick presets — the model field and slider below can be set independently of them, for any Replicate img2img model."}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
            <div style={{ width: 130, flexShrink: 0, font: "500 12.5px system-ui", color: "var(--text-dim)" }}>
              {lang === "uk" ? "Креативність" : "Creativity"}
            </div>
            <input
              type="range"
              min={0.1}
              max={0.9}
              step={0.05}
              value={settings.aiCreativity ?? 0.6}
              title={
                lang === "uk"
                  ? "Наскільки ШІ може відхилятись від оригіналу (деталізатора resemblance/сили img2img strength). Низько = обережно, високо = сміливо."
                  : "How far the AI may diverge from the original (drives the upscaler's resemblance / img2img strength). Low = cautious, high = bold."
              }
              onChange={(e) => persistDebounced({ ...settings, aiCreativity: Number(e.target.value) })}
              style={{ flex: 1 }}
            />
            <div style={{ width: 36, textAlign: "right", font: "600 12px ui-monospace, Menlo, monospace", color: "var(--text-dim)" }}>
              {(settings.aiCreativity ?? 0.6).toFixed(2)}
            </div>
          </div>
          <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", font: "500 12.5px system-ui", color: "var(--text-dim)" }}>
            <input
              type="checkbox"
              checked={Boolean(settings.aiRegenerate)}
              onChange={(e) => persist({ ...settings, aiRegenerate: e.target.checked })}
            />
            {lang === "uk" ? "✨ Режим «Нові текстури» (перемалювати, не лише деталізувати)" : "✨ “New textures” mode (repaint, not just detail)"}
          </label>
          </>
          ) : null}
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
              <button
                onClick={onShipToGame}
                disabled={!settings.gameExe}
                title={
                  lang === "uk"
                    ? "Прийняті текстури → мінімальні патчі → одразу в теку гри. Заодно встановлює й АНІМАЦІЙНІ патчі, якщо ти вже зібрав їх на вкладці «Анімації» (кнопки «💾 У патч»/«💾 Всі кліпи») — вони теж чекають у тій самій теці патчів."
                    : "Approved textures → minimal patches → straight into the game folder. Also installs any ANIMATION patches you've already built on the Animations tab (“💾 To patch”/“💾 All clips”) — they wait in the same patch folder."
                }
                style={{ padding: "8px 16px", borderRadius: 9, background: "var(--green)", border: "none", font: "700 12px system-ui", color: "#0c1f10", opacity: settings.gameExe ? 1 : 0.5 }}
              >
                {lang === "uk" ? "🚀 Патчі в гру" : "🚀 Ship to game"}
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
          <button
            onClick={() => setShowAdvancedPatch((v) => !v)}
            style={{ marginTop: 10, padding: 0, background: "none", border: "none", font: "600 11px system-ui", color: "var(--text-faint)", cursor: "pointer" }}
          >
            {showAdvancedPatch
              ? lang === "uk" ? "▾ Розширено: зібрати й встановити окремо" : "▾ Advanced: build and install separately"
              : lang === "uk" ? "▸ Розширено: зібрати й встановити окремо" : "▸ Advanced: build and install separately"}
          </button>
          {showAdvancedPatch ? (
            <div style={{ marginTop: 8, padding: 12, background: "var(--bg2)", borderRadius: 10 }}>
              <div style={{ font: "500 11.5px system-ui", color: "var(--text-faint)", marginBottom: 10, lineHeight: 1.5 }}>
                {lang === "uk"
                  ? "«🚀 Патчі в гру» вище вже робить ці два кроки одним кліком. Розділяй їх лише якщо хочеш зібрати патч-файли текстур (.pXX), не встановлюючи їх у гру одразу — наприклад щоб подивитись файли перед тим, як щось зачепити. «2️⃣ Встановити в гру» встановлює УСІ .pXX з теки «Патчі» — і текстурні, і анімаційні (з вкладки «Анімації»), не лише щойно зібрані тут."
                  : "“🚀 Ship to game” above already does both these steps in one click. Split them only if you want to build texture patch (.pXX) files without installing them into the game right away — e.g. to inspect the files before touching anything. “2️⃣ Install into game” installs EVERY .pXX in the “Patches” folder — texture AND animation ones (from the Animations tab), not just what step 1 just built."}
              </div>
              <div style={{ display: "flex", gap: 8 }}>
                <button
                  onClick={onBuildPatch}
                  title={
                    lang === "uk"
                      ? "Крок 1/2: пакує прийняті ТЕКСТУРИ у .pXX файли в теку «Патчі» вище — гру НЕ чіпає. Анімаційні патчі збираються окремо, на вкладці «Анімації»."
                      : "Step 1/2: packs approved TEXTURES into .pXX files in the “Patches” folder above — does NOT touch the game. Animation patches are built separately, on the Animations tab."
                  }
                  style={{ padding: "8px 16px", borderRadius: 9, background: "var(--accent)", border: "none", font: "600 12px system-ui", color: "#fff" }}
                >
                  {lang === "uk" ? "1️⃣ Зібрати патч текстур" : "1️⃣ Build texture patch"}
                </button>
                <button
                  onClick={onInstallPatches}
                  disabled={!settings.gameExe}
                  title={
                    lang === "uk"
                      ? "Крок 2/2: копіює ВСІ ВЖЕ ЗІБРАНІ .pXX з теки «Патчі» у теку гри — текстурні (крок 1) і анімаційні (з вкладки «Анімації») разом. Якщо нічого не зібрано — робити нічого."
                      : "Step 2/2: copies ALL ALREADY-BUILT .pXX files from the “Patches” folder into the game folder — texture ones (step 1) and animation ones (from the Animations tab) together. No-op if nothing's been built yet."
                  }
                  style={{ padding: "8px 16px", borderRadius: 9, background: "var(--bg1)", border: "1px solid var(--accent)", font: "600 12px system-ui", color: "var(--text)", opacity: settings.gameExe ? 1 : 0.5 }}
                >
                  {lang === "uk" ? "2️⃣ Встановити в гру" : "2️⃣ Install into game"}
                </button>
              </div>
            </div>
          ) : null}
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

        {isTauri() ? (
          <div style={{ background: "var(--bg1)", border: "1px solid var(--border)", borderRadius: 14, padding: 20 }}>
            <div style={{ font: "600 11px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: "var(--text-faint)", marginBottom: 12 }}>
              {lang === "uk" ? "Віддалений доступ" : "Remote access"}
            </div>
            <div style={{ font: "500 12px system-ui", color: "var(--text-faint)", marginBottom: 12, lineHeight: 1.5 }}>
              {lang === "uk"
                ? "Дай колезі посилання — воно відкриє цей застосунок у будь-якому браузері, з тими самими реальними даними, поки цей комп'ютер увімкнений і застосунок відкритий. Потребує встановленого cloudflared (безкоштовний, від Cloudflare) — застосунок його не завантажує сам."
                : "Give a colleague a link — it opens this app in any browser, with the same real data, as long as this computer is on and the app is open. Needs cloudflared installed (free, from Cloudflare) — this app doesn't download it for you."}
            </div>
            {remote?.running ? (
              <>
                <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
                  <div style={{ width: 8, height: 8, borderRadius: "50%", background: "var(--green)", flexShrink: 0 }} />
                  <div style={{ font: "600 12.5px system-ui", color: "var(--text)" }}>
                    {lang === "uk" ? "Увімкнено" : "Running"}
                  </div>
                </div>
                {remote.tunnelUrl && remote.token ? (
                  <div style={{ display: "flex", gap: 8, marginBottom: 10 }}>
                    <div
                      style={{
                        flex: 1,
                        background: "var(--bg2)",
                        border: "1px solid var(--border)",
                        borderRadius: 8,
                        padding: "9px 12px",
                        font: "500 11.5px ui-monospace, Menlo, monospace",
                        color: "var(--text)",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {`${remote.tunnelUrl}/?token=${remote.token}`}
                    </div>
                    <button
                      onClick={() => navigator.clipboard.writeText(`${remote.tunnelUrl}/?token=${remote.token}`)}
                      style={{ padding: "9px 14px", borderRadius: 8, background: "var(--accent)", border: "none", font: "600 12px system-ui", color: "#fff", whiteSpace: "nowrap" }}
                    >
                      {lang === "uk" ? "Копіювати" : "Copy"}
                    </button>
                  </div>
                ) : (
                  <div style={{ font: "500 12px system-ui", color: "var(--text-faint)", marginBottom: 10 }}>
                    {lang === "uk" ? "Створюю тунель…" : "Setting up the tunnel…"}
                  </div>
                )}
                <button
                  onClick={handleStopRemote}
                  disabled={remoteBusy}
                  style={{ padding: "8px 16px", borderRadius: 9, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 12px system-ui", color: "var(--text-dim)", opacity: remoteBusy ? 0.6 : 1 }}
                >
                  {lang === "uk" ? "Вимкнути" : "Turn off"}
                </button>
              </>
            ) : (
              <>
                {remote && !remote.cloudflaredAvailable ? (
                  <div style={{ font: "500 12px system-ui", color: "var(--red)", marginBottom: 10, lineHeight: 1.5 }}>
                    {lang === "uk"
                      ? "cloudflared не знайдено. Встанови його (github.com/cloudflare/cloudflared/releases) і спробуй ще раз — без нього доступне лише локальне вікно."
                      : "cloudflared not found. Install it (github.com/cloudflare/cloudflared/releases) and try again — without it only the local window works."}
                  </div>
                ) : null}
                <button
                  onClick={handleStartRemote}
                  disabled={remoteBusy}
                  style={{ padding: "8px 16px", borderRadius: 9, background: "var(--accent)", border: "none", font: "700 12px system-ui", color: "#fff", opacity: remoteBusy ? 0.6 : 1 }}
                >
                  {remoteBusy ? s.loading : lang === "uk" ? "🌐 Увімкнути" : "🌐 Turn on"}
                </button>
              </>
            )}
          </div>
        ) : null}
      </div>
    </div>
  );
}
