import { useEffect, useMemo, useState } from "react";
import type { Lang } from "../lib/i18n";
import { queueCount, t } from "../lib/i18n";
import type { LibraryEntry, ReviewItem } from "../lib/types";
import { listLibrary, readEditedDataUrl, readTextureDataUrl, regenerateTexture, reviewQueue, setReviewStatus } from "../lib/api";

interface Props {
  lang: Lang;
  initialPngRel: string | null;
}

type Mode = "side" | "slider";

export default function AiCompare({ lang, initialPngRel }: Props) {
  const s = t(lang);
  const [mode, setMode] = useState<Mode>("side");
  const [queue, setQueue] = useState<ReviewItem[]>([]);
  const [entries, setEntries] = useState<LibraryEntry[]>([]);
  const [index, setIndex] = useState(0);
  const [original, setOriginal] = useState<string | null>(null);
  const [variant, setVariant] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function refreshQueue(preferPngRel?: string | null) {
    const items = await reviewQueue();
    const pending = items.filter((i) => i.status !== "rejected");
    setQueue(pending);
    if (preferPngRel) {
      const i = pending.findIndex((it) => it.pngRel === preferPngRel);
      if (i >= 0) setIndex(i);
    }
  }

  useEffect(() => {
    listLibrary().then(setEntries);
    refreshQueue(initialPngRel);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const current = queue[index] ?? null;
  const entry = useMemo(() => entries.find((e) => e.pngRel === current?.pngRel) ?? null, [entries, current]);

  useEffect(() => {
    setOriginal(null);
    setVariant(null);
    if (!current) return;
    readTextureDataUrl(current.pngRel).then(setOriginal);
    readEditedDataUrl(current.pngRel).then(setVariant);
  }, [current]);

  async function act(status: "approved" | "rejected") {
    if (!current) return;
    setBusy(true);
    try {
      await setReviewStatus(current.pngRel, status);
      await refreshQueue();
      setIndex((i) => Math.min(i, Math.max(0, queue.length - 2)));
    } finally {
      setBusy(false);
    }
  }

  async function regenerateAgain() {
    if (!current) return;
    setBusy(true);
    try {
      await regenerateTexture(current.pngRel);
      const url = await readEditedDataUrl(current.pngRel);
      setVariant(url);
    } finally {
      setBusy(false);
    }
  }

  function skip() {
    setIndex((i) => (i + 1) % Math.max(1, queue.length));
  }

  if (!current || !entry) {
    return (
      <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
        {s.noSelection}
      </div>
    );
  }

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
      <div style={{ padding: "18px 26px 0", display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
        <div>
          <div style={{ font: "700 18px system-ui" }}>{entry.name}</div>
          <div style={{ font: "500 11px ui-monospace, Menlo, monospace", color: "var(--text-faint)", marginTop: 3 }}>
            {entry.folder}
          </div>
        </div>
        <div style={{ display: "flex", background: "var(--bg2)", border: "1px solid var(--border)", borderRadius: 20, padding: 3, gap: 2 }}>
          {(["side", "slider"] as Mode[]).map((m) => (
            <button
              key={m}
              onClick={() => setMode(m)}
              style={{
                padding: "5px 14px",
                borderRadius: 16,
                border: "none",
                font: "600 11px system-ui",
                background: mode === m ? "var(--accent)" : "transparent",
                color: mode === m ? "#fff" : "var(--text-faint)",
              }}
            >
              {m}
            </button>
          ))}
        </div>
      </div>

      {mode === "side" ? (
        <div style={{ flex: 1, display: "flex", gap: 18, padding: "18px 26px", minHeight: 0 }}>
          {[
            [s.original, original, "var(--border-strong)"],
            [s.variant, variant, "var(--accent)"],
          ].map(([label, url, border], i) => (
            <div key={i} style={{ flex: 1, display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
              <div style={{ font: "600 11px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: i === 1 ? "var(--accent)" : "var(--text-faint)" }}>
                {label}
              </div>
              <div
                style={{
                  flex: 1,
                  borderRadius: 14,
                  border: `1px solid ${border}`,
                  background: url ? `center / contain no-repeat var(--bg0) url(${url})` : "var(--bg2)",
                }}
              />
            </div>
          ))}
        </div>
      ) : (
        <div style={{ flex: 1, padding: "18px 26px", minHeight: 0 }}>
          <div style={{ position: "relative", height: "100%", borderRadius: 14, overflow: "hidden", border: "1px solid var(--border-strong)" }}>
            <div style={{ position: "absolute", inset: 0, background: variant ? `center / contain no-repeat var(--bg0) url(${variant})` : "var(--bg2)" }} />
            <div
              style={{
                position: "absolute",
                inset: 0,
                width: "42%",
                overflow: "hidden",
                background: original ? `center / contain no-repeat var(--bg0) url(${original})` : "var(--bg2)",
                borderRight: "2px solid var(--accent)",
              }}
            />
            <div style={{ position: "absolute", top: 14, left: 14, font: "700 10px system-ui", color: "var(--text-dim)", background: "rgba(0,0,0,.4)", padding: "4px 9px", borderRadius: 6 }}>
              {s.original}
            </div>
            <div style={{ position: "absolute", top: 14, right: 14, font: "700 10px system-ui", color: "#fff", background: "var(--accent)", padding: "4px 9px", borderRadius: 6 }}>
              {s.variant}
            </div>
          </div>
        </div>
      )}

      <div style={{ flexShrink: 0, padding: "16px 26px 22px", display: "flex", alignItems: "center", gap: 12, borderTop: "1px solid var(--border)" }}>
        <button disabled={busy} onClick={() => act("approved")} style={{ padding: "11px 22px", borderRadius: 10, background: "var(--green)", border: "none", font: "700 13px system-ui", color: "#0c1f10" }}>
          {s.approve}
        </button>
        <button disabled={busy} onClick={regenerateAgain} style={{ padding: "11px 22px", borderRadius: 10, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 13px system-ui", color: "var(--text)" }}>
          {s.regenerate}
        </button>
        <button disabled={busy} onClick={skip} style={{ padding: "11px 22px", borderRadius: 10, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 13px system-ui", color: "var(--text-dim)" }}>
          {s.skip}
        </button>
        <div style={{ flex: 1 }} />
        <button disabled={busy} onClick={() => act("rejected")} style={{ padding: "11px 22px", borderRadius: 10, background: "transparent", border: "1px solid var(--red)", font: "700 13px system-ui", color: "var(--red)" }}>
          {s.reject}
        </button>
        <div style={{ width: 1, height: 26, background: "var(--border)", margin: "0 6px" }} />
        <div style={{ font: "500 12px system-ui", color: "var(--text-faint)" }}>{queueCount(queue.length, lang)}</div>
      </div>

      <div style={{ flexShrink: 0, display: "flex", gap: 8, padding: "0 26px 20px", overflow: "auto" }}>
        {queue.map((q, i) => (
          <div
            key={q.pngRel}
            onClick={() => setIndex(i)}
            style={{
              width: 56,
              height: 56,
              borderRadius: 9,
              flexShrink: 0,
              border: i === index ? "2px solid var(--accent)" : "1px solid var(--border)",
              background: "var(--bg2)",
              cursor: "pointer",
            }}
          />
        ))}
      </div>
    </div>
  );
}
