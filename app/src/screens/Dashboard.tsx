import { useEffect, useState } from "react";
import type { Lang } from "../lib/i18n";
import { getStats } from "../lib/api";
import { formatBytes } from "../lib/library";
import type { AppStats } from "../lib/types";

interface Props {
  lang: Lang;
}

function Card({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div style={{ background: "var(--bg1)", border: "1px solid var(--border)", borderRadius: 14, padding: 20, flex: 1, minWidth: 160 }}>
      <div style={{ font: "600 11px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: "var(--text-faint)" }}>
        {label}
      </div>
      <div style={{ font: "700 26px system-ui", color: "var(--text)", marginTop: 8 }}>{value}</div>
      {sub ? <div style={{ font: "500 12px system-ui", color: "var(--text-faint)", marginTop: 4 }}>{sub}</div> : null}
    </div>
  );
}

export default function Dashboard({ lang }: Props) {
  const [stats, setStats] = useState<AppStats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getStats()
      .then(setStats)
      .catch((e) => setError(String(e)));
  }, []);

  const uk = lang === "uk";

  return (
    <div style={{ flex: 1, overflow: "auto", padding: "30px 40px" }}>
      <div style={{ font: "700 20px system-ui", marginBottom: 4 }}>{uk ? "Дашборд" : "Dashboard"}</div>
      <div style={{ font: "500 13px system-ui", color: "var(--text-faint)", marginBottom: 24 }}>
        {uk ? "Реальний стан проєкту — жодних вигаданих чисел." : "Real project state — no invented numbers."}
      </div>

      {error ? <div style={{ color: "var(--red)", marginBottom: 16 }}>{error}</div> : null}

      {!stats ? (
        <div style={{ color: "var(--text-faint)" }}>{uk ? "Завантаження…" : "Loading…"}</div>
      ) : (
        <>
          <div style={{ display: "flex", gap: 16, flexWrap: "wrap", marginBottom: 16 }}>
            <Card
              label={uk ? "Текстури опрацьовано" : "Textures processed"}
              value={`${stats.textureProcessed} / ${stats.textureTotal}`}
              sub={stats.textureTotal > 0 ? `${Math.round((stats.textureProcessed / stats.textureTotal) * 100)}%` : undefined}
            />
            <Card
              label={uk ? "Моделі доступні" : "Models available"}
              value={String(stats.modelsAvailable)}
              sub={uk ? "реальні меші з гри" : "real meshes from the game"}
            />
            <Card
              label={uk ? "Архіви гри" : "Game archives"}
              value={stats.archiveCount != null ? String(stats.archiveCount) : "—"}
              sub={uk ? "гру не підключено" : undefined}
            />
          </div>

          <div style={{ display: "flex", gap: 16, flexWrap: "wrap", marginBottom: 16 }}>
            <Card
              label={uk ? "Розмір гри (архіви)" : "Game size (archives)"}
              value={stats.gameArchiveTotalBytes != null ? formatBytes(stats.gameArchiveTotalBytes) : "—"}
            />
            <Card label={uk ? "Розмір поточного виводу" : "Current output size"} value={formatBytes(stats.outputDirSizeBytes)} />
          </div>

          <div style={{ background: "var(--bg1)", border: "1px solid var(--border)", borderRadius: 14, padding: 20 }}>
            <div style={{ font: "600 11px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: "var(--text-faint)", marginBottom: 12 }}>
              {uk ? "Система" : "System"}
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 8, font: "500 13px system-ui", color: "var(--text-dim)" }}>
              <div style={{ width: 8, height: 8, borderRadius: "50%", background: "var(--green)" }} />
              {uk ? "Бекенд відповідає" : "Backend responding"}
            </div>
            <div style={{ font: "500 12px ui-monospace, Menlo, monospace", color: "var(--text-faint)", marginTop: 8 }}>
              RisenLab v{stats.appVersion}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
