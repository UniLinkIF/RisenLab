import { useEffect, useState } from "react";
import type { Lang } from "../lib/i18n";
import { t } from "../lib/i18n";
import type { LibraryEntry, ReviewStatus, TextureMeta } from "../lib/types";
import { readTextureDataUrl, textureMeta } from "../lib/api";
import { formatBytes } from "../lib/library";

interface Props {
  entry: LibraryEntry | null;
  lang: Lang;
  onRegenerate: (entry: LibraryEntry) => void;
  regenerating: boolean;
  /** Set when this entry has ever been through AI enhancement (any status) — drives the
   * "Переглянути до/після" button below. `undefined`/absent = never touched. */
  reviewStatus?: ReviewStatus;
  /** Reopens the before/after compare screen for an entry that's already been decided
   * (approved, or still pending) — the owner's fix for "I can't look at a texture's before/
   * after again once I've left the review queue". Not offered for "rejected": the `edited/`
   * variant is deleted server-side the moment a texture is rejected, so there's nothing left
   * to show. */
  onViewCompare: (entry: LibraryEntry) => void;
}

export default function DetailPanel({ entry, lang, onRegenerate, regenerating, reviewStatus, onViewCompare }: Props) {
  const s = t(lang);
  const [preview, setPreview] = useState<string | null>(null);
  const [meta, setMeta] = useState<TextureMeta | null>(null);

  useEffect(() => {
    setPreview(null);
    setMeta(null);
    if (!entry) return;
    let cancelled = false;
    readTextureDataUrl(entry.pngRel).then((url) => {
      if (!cancelled) setPreview(url);
    });
    textureMeta(entry.archivePath, entry.entryPath).then((m) => {
      if (!cancelled) setMeta(m);
    });
    return () => {
      cancelled = true;
    };
  }, [entry]);

  const rows: Array<[string, string]> = meta
    ? [
        [lang === "uk" ? "Розмір" : "Size", `${meta.width} × ${meta.height}`],
        [lang === "uk" ? "Формат" : "Format", meta.pixelFormat],
        [lang === "uk" ? "Розмір файлу" : "File size", formatBytes(meta.fileSize)],
      ]
    : [];

  return (
    <div
      style={{
        width: 300,
        flexShrink: 0,
        background: "var(--bg1)",
        borderLeft: "1px solid var(--border)",
        padding: 20,
        overflow: "auto",
      }}
    >
      <div
        style={{
          borderRadius: 12,
          height: 220,
          border: "1px solid var(--border-strong)",
          background: preview ? `center / cover no-repeat url(${preview})` : "var(--bg2)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        {!entry ? (
          <div style={{ font: "600 11px ui-monospace, Menlo, monospace", color: "var(--text-faint)" }}>
            {s.noSelection}
          </div>
        ) : null}
      </div>

      {entry ? (
        <>
          <div style={{ font: "700 14px system-ui", color: "var(--text)", marginTop: 16 }}>{entry.name}</div>
          <div style={{ font: "500 11px ui-monospace, Menlo, monospace", color: "var(--text-faint)", marginTop: 3 }}>
            {entry.entryPath}
          </div>

          <div style={{ marginTop: 16 }}>
            {rows.map(([k, v]) => (
              <div
                key={k}
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  padding: "8px 0",
                  borderBottom: "1px solid var(--border)",
                }}
              >
                <div style={{ font: "500 12px system-ui", color: "var(--text-faint)" }}>{k}</div>
                <div style={{ font: "600 12px ui-monospace, Menlo, monospace", color: "var(--text-dim)" }}>{v}</div>
              </div>
            ))}
          </div>

          <div style={{ display: "flex", gap: 8, marginTop: 16 }}>
            <div
              style={{
                flex: 1,
                textAlign: "center",
                padding: 10,
                borderRadius: 9,
                background: "var(--accent)",
                font: "600 12.5px system-ui",
                color: "#fff",
              }}
            >
              {s.btnExtract}
            </div>
            <button
              disabled={regenerating}
              onClick={() => onRegenerate(entry)}
              style={{
                flex: 1,
                textAlign: "center",
                padding: 10,
                borderRadius: 9,
                background: "var(--bg2)",
                border: "1px solid var(--border)",
                font: "600 12.5px system-ui",
                color: "var(--text)",
                opacity: regenerating ? 0.6 : 1,
              }}
            >
              {regenerating ? s.loading : s.btnRegenerate}
            </button>
          </div>
          {reviewStatus && reviewStatus !== "rejected" ? (
            <button
              onClick={() => onViewCompare(entry)}
              style={{
                width: "100%",
                marginTop: 8,
                padding: 10,
                borderRadius: 9,
                background: "var(--bg2)",
                border: "1px solid var(--accent)",
                font: "600 12.5px system-ui",
                color: "var(--accent)",
              }}
            >
              {lang === "uk" ? "👁 Переглянути до/після" : "👁 View before/after"}
            </button>
          ) : null}
        </>
      ) : null}
    </div>
  );
}
