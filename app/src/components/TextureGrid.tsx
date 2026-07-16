import { useEffect, useState } from "react";
import type { Lang } from "../lib/i18n";
import type { LibraryEntry, ReviewStatus } from "../lib/types";
import { readTextureDataUrl } from "../lib/api";
import { badgeForStatus } from "../lib/library";

interface Props {
  entries: LibraryEntry[];
  statusByPngRel: Map<string, ReviewStatus>;
  selected: LibraryEntry | null;
  onSelect: (e: LibraryEntry) => void;
  lang: Lang;
  columns?: number;
}

function Thumb({ entry }: { entry: LibraryEntry }) {
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setSrc(null);
    readTextureDataUrl(entry.pngRel).then((url) => {
      if (!cancelled) setSrc(url);
    });
    return () => {
      cancelled = true;
    };
  }, [entry.pngRel]);

  return (
    <div
      style={{
        height: 120,
        flexShrink: 0,
        position: "relative",
        background: "var(--bg2)",
        backgroundImage: src ? `url(${src})` : undefined,
        backgroundSize: "cover",
        backgroundPosition: "center",
      }}
    />
  );
}

export default function TextureGrid({ entries, statusByPngRel, selected, onSelect, lang, columns = 4 }: Props) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: `repeat(${columns}, 1fr)`, gap: 14 }}>
      {entries.map((e) => {
        const badge = badgeForStatus(statusByPngRel.get(e.pngRel), lang);
        const isSelected = selected?.pngRel === e.pngRel;
        return (
          <div
            key={e.pngRel}
            onClick={() => onSelect(e)}
            style={{
              background: "var(--bg1)",
              border: `1px solid ${isSelected ? "var(--accent)" : "var(--border)"}`,
              borderRadius: 12,
              overflow: "hidden",
              display: "flex",
              flexDirection: "column",
              cursor: "pointer",
            }}
          >
            <div style={{ position: "relative" }}>
              <Thumb entry={e} />
              {badge ? (
                <div
                  style={{
                    position: "absolute",
                    top: 8,
                    left: 8,
                    font: "700 9px system-ui",
                    letterSpacing: ".03em",
                    textTransform: "uppercase",
                    background: badge.background,
                    color: "#fff",
                    padding: "3px 7px",
                    borderRadius: 5,
                  }}
                >
                  {badge.label}
                </div>
              ) : null}
            </div>
            <div style={{ padding: "9px 11px 11px" }}>
              <div
                style={{
                  font: "600 12px ui-monospace, Menlo, monospace",
                  color: "var(--text)",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {e.name}
              </div>
              <div
                style={{
                  font: "500 11px system-ui",
                  color: "var(--text-faint)",
                  marginTop: 3,
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {e.folder || e.archiveStem}
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
