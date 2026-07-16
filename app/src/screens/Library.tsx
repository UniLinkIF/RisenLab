import { useEffect, useMemo, useState } from "react";
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
}

// Each grid card fetches its own thumbnail independently (see TextureGrid's Thumb component)
// and there's no virtualization — with the real library (1300+ textures), rendering every
// match at once causes hundreds of concurrent requests and a very heavy DOM, badly stalling
// the UI. Capping the grid and pointing the user at narrowing their search/folder keeps every
// view responsive; full virtualization would remove the cap but isn't needed at this scale.
const GRID_LIMIT = 150;

export default function Library({ lang, onRegenerated }: Props) {
  const s = t(lang);
  const [entries, setEntries] = useState<LibraryEntry[]>([]);
  const [treeKey, setTreeKey] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [hide2D, setHide2D] = useState(false);
  const [selected, setSelected] = useState<LibraryEntry | null>(null);
  const [statusByPngRel, setStatusByPngRel] = useState<Map<string, ReviewStatus>>(new Map());
  const [regenerating, setRegenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
      onRegenerated(entry);
    } catch (e) {
      setError(String(e));
    } finally {
      setRegenerating(false);
    }
  }

  return (
    <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
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
    </div>
  );
}
