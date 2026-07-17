import { useEffect, useMemo, useRef, useState } from "react";
import type { Lang } from "../lib/i18n";
import { queueCount, t } from "../lib/i18n";
import type { LibraryEntry, ReviewItem } from "../lib/types";
import { listLibrary, readEditedDataUrl, readTextureDataUrl, regenerateTexture, reviewQueue, setReviewStatus } from "../lib/api";
import { findTextureEntryForBaseName } from "../lib/materials";
import Model3DViewer from "../components/Model3DViewer";

interface Props {
  lang: Lang;
  initialPngRel: string | null;
  /** When the review was opened from Models we know which mesh the texture belongs to —
   * enables the 3D before/after mode. Null when opened from the Library. */
  modelObjUrl?: string | null;
}

type Mode = "side" | "slider" | "3d";

export default function AiCompare({ lang, initialPngRel, modelObjUrl }: Props) {
  const s = t(lang);
  const [mode, setMode] = useState<Mode>("side");
  const [queue, setQueue] = useState<ReviewItem[]>([]);
  const [entries, setEntries] = useState<LibraryEntry[]>([]);
  const [index, setIndex] = useState(0);
  const [original, setOriginal] = useState<string | null>(null);
  const [variant, setVariant] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Slider-compare divider position (0..1). Driven by pointer drag/click on the container.
  const [sliderPos, setSliderPos] = useState(0.5);
  const draggingRef = useRef(false);
  const sliderBoxRef = useRef<HTMLDivElement | null>(null);

  function moveSlider(clientX: number) {
    const box = sliderBoxRef.current?.getBoundingClientRect();
    if (!box || box.width === 0) return;
    setSliderPos(Math.min(1, Math.max(0, (clientX - box.left) / box.width)));
  }

  async function refreshQueue(preferPngRel?: string | null): Promise<ReviewItem[]> {
    const items = await reviewQueue();
    // Pending ONLY: an approved texture must leave the review queue immediately — keeping it
    // (the old `!== "rejected"` filter) meant "Прийняти" visibly did nothing, because the
    // just-approved item stayed as the current one.
    const pending = items.filter((i) => i.status === "pending");
    setQueue(pending);
    if (preferPngRel) {
      const i = pending.findIndex((it) => it.pngRel === preferPngRel);
      if (i >= 0) setIndex(i);
    }
    return pending;
  }

  useEffect(() => {
    listLibrary().then(setEntries);
    refreshQueue(initialPngRel);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const current = queue[index] ?? null;

  // 3D mode: two viewers of the SAME mesh where only the reviewed texture differs — every
  // other material resolves normally, so multi-material models stay correct.
  const makeResolver = (variantUrl: string | null) => async (baseName: string) => {
    const found = findTextureEntryForBaseName(entries, baseName);
    if (!found) return null;
    if (current && found.pngRel === current.pngRel) return variantUrl;
    return readTextureDataUrl(found.pngRel);
  };
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
      // Clamp against the FRESH queue length — the old code clamped against the stale
      // closure's `queue`, which could leave the index past the end after the last item.
      const next = await refreshQueue();
      setIndex((i) => Math.min(i, Math.max(0, next.length - 1)));
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
      // Cache-bust: the dev API serves the edited file from a STABLE url, so after a
      // re-generate the browser happily shows its cached old image — the button looked
      // completely dead even though the file on disk changed. (Tauri returns data: URLs,
      // which are self-busting — only query-style urls need the extra param.)
      setVariant(url.startsWith("data:") ? url : `${url}${url.includes("?") ? "&" : "?"}v=${Date.now()}`);
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
          {((modelObjUrl ? ["side", "slider", "3d"] : ["side", "slider"]) as Mode[]).map((m) => (
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

      {mode === "3d" && modelObjUrl ? (
        <div style={{ flex: 1, display: "flex", gap: 2, padding: "18px 26px", minHeight: 0 }}>
          {([
            [s.original, original, false],
            [s.variant, variant, true],
          ] as [string, string | null, boolean][]).map(([label, url, isVariant]) => (
            <div key={label} style={{ flex: 1, position: "relative", minWidth: 0, borderRadius: 14, overflow: "hidden", border: `1px solid ${isVariant ? "var(--accent)" : "var(--border-strong)"}` }}>
              <Model3DViewer
                key={`${current.pngRel}::${isVariant ? "v" : "o"}::${url ?? ""}`}
                objUrl={modelObjUrl}
                diffuseUrl={url}
                normalUrl={null}
                mode="textured"
                resolveTexture={makeResolver(url)}
              />
              <div style={{ position: "absolute", top: 10, left: 10, font: "700 10px system-ui", color: isVariant ? "#fff" : "var(--text-dim)", background: isVariant ? "var(--accent)" : "rgba(0,0,0,.45)", padding: "4px 9px", borderRadius: 6, pointerEvents: "none" }}>
                {label}
              </div>
            </div>
          ))}
        </div>
      ) : mode === "side" ? (
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
          <div
            ref={sliderBoxRef}
            onPointerDown={(e) => {
              draggingRef.current = true;
              (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
              moveSlider(e.clientX);
            }}
            onPointerMove={(e) => {
              if (draggingRef.current) moveSlider(e.clientX);
            }}
            onPointerUp={() => {
              draggingRef.current = false;
            }}
            style={{ position: "relative", height: "100%", borderRadius: 14, overflow: "hidden", border: "1px solid var(--border-strong)", cursor: "ew-resize", touchAction: "none", userSelect: "none" }}
          >
            <div style={{ position: "absolute", inset: 0, background: variant ? `center / contain no-repeat var(--bg0) url(${variant})` : "var(--bg2)" }} />
            {/* Full-size layer clipped from the right — both backgrounds scale/center
                identically, so the two images stay perfectly aligned at any divider position
                (the old fixed-width layer centered its image differently and never moved —
                the real "свайп не працює" bug). */}
            <div
              style={{
                position: "absolute",
                inset: 0,
                background: original ? `center / contain no-repeat var(--bg0) url(${original})` : "var(--bg2)",
                clipPath: `inset(0 ${(1 - sliderPos) * 100}% 0 0)`,
              }}
            />
            <div style={{ position: "absolute", top: 0, bottom: 0, left: `calc(${sliderPos * 100}% - 1px)`, width: 2, background: "var(--accent)", pointerEvents: "none" }} />
            <div
              style={{
                position: "absolute",
                top: "50%",
                left: `${sliderPos * 100}%`,
                transform: "translate(-50%, -50%)",
                width: 26,
                height: 26,
                borderRadius: "50%",
                background: "var(--accent)",
                color: "#fff",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                font: "700 12px system-ui",
                pointerEvents: "none",
                boxShadow: "0 1px 6px rgba(0,0,0,.4)",
              }}
            >
              ⇔
            </div>
            <div style={{ position: "absolute", top: 14, left: 14, font: "700 10px system-ui", color: "var(--text-dim)", background: "rgba(0,0,0,.4)", padding: "4px 9px", borderRadius: 6, pointerEvents: "none" }}>
              {s.original}
            </div>
            <div style={{ position: "absolute", top: 14, right: 14, font: "700 10px system-ui", color: "#fff", background: "var(--accent)", padding: "4px 9px", borderRadius: 6, pointerEvents: "none" }}>
              {s.variant}
            </div>
          </div>
        </div>
      )}

      <div style={{ flexShrink: 0, padding: "16px 26px 22px", display: "flex", alignItems: "center", gap: 12, borderTop: "1px solid var(--border)" }}>
        <button disabled={busy} onClick={() => act("approved")} style={{ padding: "11px 22px", borderRadius: 10, background: "var(--green)", border: "none", font: "700 13px system-ui", color: "#0c1f10" }}>
          {s.approve}
        </button>
        <button disabled={busy} onClick={regenerateAgain} style={{ padding: "11px 22px", borderRadius: 10, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 13px system-ui", color: busy ? "var(--text-faint)" : "var(--text)", cursor: busy ? "wait" : "pointer" }}>
          {busy ? (lang === "uk" ? "Працюю… (ШІ до 1-2 хв)" : "Working… (AI up to 1-2 min)") : s.regenerate}
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
