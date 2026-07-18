import { useEffect, useMemo, useRef, useState } from "react";
import type { Lang } from "../lib/i18n";
import { t } from "../lib/i18n";
import type { LibraryEntry, ReviewStatus } from "../lib/types";
import { listLibrary, regenerateTexture, reviewQueue } from "../lib/api";
import { buildFolderTree, countProcessed, filterEntries, filterByTreeKey, isFlat2DOnly } from "../lib/library";
import FolderTree from "../components/FolderTree";
import TextureGrid from "../components/TextureGrid";
import DetailPanel from "../components/DetailPanel";

interface Props {
  lang: Lang;
  onRegenerated: (entry: LibraryEntry) => void;
  /** Called after each successful regenerate (single or one batch item) — a cheap "the pending
   * count just changed" ping for App's persistent Titlebar badge, NOT a navigation request. */
  onQueueChanged: () => void;
  /** Opens the review queue with no specific entry preferred (starts at the first pending
   * item) — used by the batch-summary toast, where no single texture is "the" one to jump to. */
  onOpenReviewQueue: () => void;
}

// Each grid card fetches its own thumbnail independently (see TextureGrid's Thumb component)
// and there's no virtualization — with the real library (1300+ textures), rendering every
// match at once causes hundreds of concurrent requests and a very heavy DOM, badly stalling
// the UI. Capping the grid and pointing the user at narrowing their search/folder keeps every
// view responsive; full virtualization would remove the cap but isn't needed at this scale.
const GRID_LIMIT = 150;

/** Progress of a running batch enhancement — the remaster workflow's "process a whole
 * folder/creature at once" step (each texture still goes through the normal review queue). */
interface BatchProgress {
  done: number;
  total: number;
  skipped: number;
  failed: number;
  currentName: string | null;
}

export default function Library({ lang, onRegenerated, onQueueChanged, onOpenReviewQueue }: Props) {
  const s = t(lang);
  const [entries, setEntries] = useState<LibraryEntry[]>([]);
  const [treeKey, setTreeKey] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [hide2D, setHide2D] = useState(false);
  const [selected, setSelected] = useState<LibraryEntry | null>(null);
  const [statusByPngRel, setStatusByPngRel] = useState<Map<string, ReviewStatus>>(new Map());
  const [regenerating, setRegenerating] = useState(false);
  const [batch, setBatch] = useState<BatchProgress | null>(null);
  const batchCancelled = useRef(false);
  const [error, setError] = useState<string | null>(null);
  // Non-blocking "landed in the review queue" notices (owner: "не хочу щоб мені відкривало
  // погодження всіх 1000 які в черзі" — see Models.tsx's identical fix). Neither ever
  // auto-navigates; both just offer a button the user can ignore.
  const [genNotice, setGenNotice] = useState<LibraryEntry | null>(null);
  const [batchDoneNotice, setBatchDoneNotice] = useState<number | null>(null);

  useEffect(() => {
    listLibrary()
      .then(setEntries)
      .catch((e) => setError(String(e)));
    reviewQueue()
      .then((items) => setStatusByPngRel(new Map(items.map((i) => [i.pngRel, i.status]))))
      .catch(() => {});
  }, []);

  const tree = useMemo(() => buildFolderTree(entries), [entries]);
  const visible = useMemo(() => {
    const byTree = filterByTreeKey(entries, treeKey);
    const byQuery = filterEntries(byTree, query);
    return hide2D ? byQuery.filter((e) => !isFlat2DOnly(e)) : byQuery;
  }, [entries, treeKey, query, hide2D]);

  async function handleRegenerate(entry: LibraryEntry) {
    setRegenerating(true);
    try {
      await regenerateTexture(entry.pngRel);
      setStatusByPngRel((prev) => new Map(prev).set(entry.pngRel, "pending"));
      onQueueChanged();
      // Deliberately NO auto-navigation (same fix as Models.tsx) — the result lands in the
      // review queue quietly, a toast offers the jump.
      setGenNotice(entry);
    } catch (e) {
      setError(String(e));
    } finally {
      setRegenerating(false);
    }
  }

  // Enhances every texture the current folder/search filter shows, sequentially (each call
  // shells to the CLI — and with an AI key configured, to the AI provider — so parallelism
  // buys little and interleaves errors). Already-processed textures (any review status) are
  // skipped: re-running a folder resumes where it left off instead of redoing paid AI calls.
  async function handleBatchEnhance() {
    const targets = visible;
    if (targets.length === 0 || batch) return;
    batchCancelled.current = false;
    let done = 0;
    let skipped = 0;
    let failed = 0;
    setError(null);
    setBatchDoneNotice(null);
    setBatch({ done, total: targets.length, skipped, failed, currentName: null });
    for (const entry of targets) {
      if (batchCancelled.current) break;
      if (statusByPngRel.has(entry.pngRel)) {
        skipped++;
        setBatch({ done, total: targets.length, skipped, failed, currentName: null });
        continue;
      }
      setBatch({ done, total: targets.length, skipped, failed, currentName: entry.name });
      try {
        await regenerateTexture(entry.pngRel);
        setStatusByPngRel((prev) => new Map(prev).set(entry.pngRel, "pending"));
        // Cheap local bump only — NOT onRegenerated (that would navigate away, and doing it up
        // to `targets.length` times in a row is exactly the "1000 textures yanked me into
        // review while I was busy in Models" bug the owner reported. One summary notice with
        // an explicit button goes out once the whole batch finishes, below.).
        onQueueChanged();
        done++;
      } catch {
        // Keep going: one bad texture (or one flaky AI call) must not kill a 200-item run —
        // the count is surfaced and the texture stays unprocessed for a later re-run.
        failed++;
      }
      setBatch({ done, total: targets.length, skipped, failed, currentName: null });
    }
    setBatch(null);
    if (failed > 0) {
      setError(
        lang === "uk"
          ? `Покращено ${done}, пропущено ${skipped}, з помилкою ${failed} — запусти ще раз, щоб повторити невдалі.`
          : `Enhanced ${done}, skipped ${skipped}, failed ${failed} — run again to retry the failures.`,
      );
    }
    if (done > 0) setBatchDoneNotice(done);
  }

  return (
    <div style={{ flex: 1, display: "flex", minHeight: 0, position: "relative" }}>
      <FolderTree nodes={tree} selectedKey={treeKey} onSelect={setTreeKey} title={s.archives} />

      <div style={{ flex: 1, overflow: "auto", padding: "20px 22px", minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 16 }}>
          <div>
            <div style={{ font: "700 18px system-ui", color: "var(--text)" }}>{s.libraryTitle}</div>
            <div style={{ font: "500 12px ui-monospace, Menlo, monospace", color: "var(--text-faint)", marginTop: 2 }}>
              {treeKey ?? `${entries.length} ${s.textures}`}
              {entries.length > 0 ? ` · ${countProcessed(entries, statusByPngRel)}/${entries.length} ${lang === "uk" ? "оброблено" : "processed"}` : ""}
            </div>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            {batch ? (
              <>
                <div style={{ font: "600 12px system-ui", color: "var(--accent)", whiteSpace: "nowrap" }}>
                  {lang === "uk"
                    ? `Покращення ${batch.done + batch.skipped + batch.failed}/${batch.total}…`
                    : `Enhancing ${batch.done + batch.skipped + batch.failed}/${batch.total}…`}
                  {batch.currentName ? ` (${batch.currentName})` : ""}
                </div>
                <button
                  onClick={() => {
                    batchCancelled.current = true;
                  }}
                  style={{
                    padding: "7px 14px",
                    borderRadius: 16,
                    background: "var(--bg2)",
                    border: "1px solid var(--border)",
                    font: "600 11.5px system-ui",
                    color: "var(--text-dim)",
                    whiteSpace: "nowrap",
                  }}
                >
                  {lang === "uk" ? "Зупинити" : "Stop"}
                </button>
              </>
            ) : (
              <button
                onClick={handleBatchEnhance}
                disabled={visible.length === 0}
                title={
                  lang === "uk"
                    ? "Покращити всі текстури поточної папки/пошуку (вже оброблені пропускаються — можна безпечно перезапускати). Кожна проходить звичайне рев'ю перед патчем."
                    : "Enhance every texture in the current folder/search (already-processed are skipped — safe to re-run). Each goes through the normal review before patching."
                }
                style={{
                  padding: "7px 14px",
                  borderRadius: 16,
                  background: "var(--accent)",
                  border: "1px solid var(--accent)",
                  font: "600 11.5px system-ui",
                  color: "#fff",
                  whiteSpace: "nowrap",
                  opacity: visible.length === 0 ? 0.5 : 1,
                }}
              >
                {lang === "uk" ? `✨ Покращити всі (${visible.length})` : `✨ Enhance all (${visible.length})`}
              </button>
            )}
            <label style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer", font: "500 12px system-ui", color: "var(--text-dim)", whiteSpace: "nowrap" }}>
              <input type="checkbox" checked={hide2D} onChange={(e) => setHide2D(e.target.checked)} />
              {lang === "uk" ? "Приховати текстури 2D" : "Hide 2D textures"}
            </label>
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={s.searchPlaceholder}
              style={{
                background: "var(--bg2)",
                border: "1px solid var(--border)",
                borderRadius: 9,
                padding: "8px 14px",
                width: 260,
                color: "var(--text)",
                font: "500 13px system-ui",
              }}
            />
          </div>
        </div>

        {error ? <div style={{ color: "var(--red)", marginBottom: 12 }}>{error}</div> : null}

        <TextureGrid
          entries={visible.slice(0, GRID_LIMIT)}
          statusByPngRel={statusByPngRel}
          selected={selected}
          onSelect={setSelected}
          lang={lang}
        />
        {visible.length > GRID_LIMIT ? (
          <div style={{ padding: "16px 4px", font: "500 12px system-ui", color: "var(--text-faint)" }}>
            {lang === "uk"
              ? `+${visible.length - GRID_LIMIT} ще — звузьте пошук або папку, щоб побачити більше`
              : `+${visible.length - GRID_LIMIT} more — narrow the search or folder to see more`}
          </div>
        ) : null}
      </div>

      <DetailPanel entry={selected} lang={lang} onRegenerate={handleRegenerate} regenerating={regenerating} />

      {genNotice ? (
        <div
          style={{
            position: "absolute",
            bottom: 14,
            right: 14,
            zIndex: 50,
            display: "flex",
            alignItems: "center",
            gap: 10,
            background: "var(--bg1)",
            border: "1px solid var(--border-strong)",
            borderRadius: 10,
            padding: "10px 14px",
            boxShadow: "0 6px 20px rgba(0,0,0,.4)",
            font: "500 12px system-ui",
            color: "var(--text)",
          }}
        >
          {lang === "uk" ? "✓ Додано в чергу рев'ю" : "✓ Added to the review queue"}
          <button
            onClick={() => onRegenerated(genNotice)}
            style={{ padding: "6px 12px", borderRadius: 8, background: "var(--accent)", border: "none", font: "600 11.5px system-ui", color: "#fff" }}
          >
            {lang === "uk" ? "Відкрити рев'ю" : "Open review"}
          </button>
          <button
            onClick={() => setGenNotice(null)}
            style={{ padding: "6px 10px", borderRadius: 8, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 11.5px system-ui", color: "var(--text-dim)" }}
          >
            {lang === "uk" ? "Пізніше" : "Later"}
          </button>
        </div>
      ) : null}

      {batchDoneNotice ? (
        <div
          style={{
            position: "absolute",
            bottom: 14,
            right: 14,
            zIndex: 50,
            display: "flex",
            alignItems: "center",
            gap: 10,
            background: "var(--bg1)",
            border: "1px solid var(--border-strong)",
            borderRadius: 10,
            padding: "10px 14px",
            boxShadow: "0 6px 20px rgba(0,0,0,.4)",
            font: "500 12px system-ui",
            color: "var(--text)",
          }}
        >
          {lang === "uk" ? `✓ Покращено ${batchDoneNotice} — усі в черзі рев'ю` : `✓ Enhanced ${batchDoneNotice} — all in the review queue`}
          <button
            onClick={() => {
              setBatchDoneNotice(null);
              onOpenReviewQueue();
            }}
            style={{ padding: "6px 12px", borderRadius: 8, background: "var(--accent)", border: "none", font: "600 11.5px system-ui", color: "#fff" }}
          >
            {lang === "uk" ? "Відкрити рев'ю" : "Open review"}
          </button>
          <button
            onClick={() => setBatchDoneNotice(null)}
            style={{ padding: "6px 10px", borderRadius: 8, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 11.5px system-ui", color: "var(--text-dim)" }}
          >
            {lang === "uk" ? "Пізніше" : "Later"}
          </button>
        </div>
      ) : null}
    </div>
  );
}
