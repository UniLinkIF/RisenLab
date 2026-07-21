import { useCallback, useEffect, useMemo, useState } from "react";
import type { Lang } from "../lib/i18n";
import type { LibraryEntry, MeshEntry, MotionEntry } from "../lib/types";
import { listLibrary, listMeshes, listMotions, meshObjUrl, readTextureDataUrl } from "../lib/api";
import { filterEntries } from "../lib/library";
import { findTextureEntryForBaseName } from "../lib/materials";
import { categorizeMesh, type ItemZoneId } from "../lib/showroomCategorize";
import { deriveScenarios, matchScenariosForItemName } from "../lib/scenarios";
import Model3DViewer, { type ViewMode } from "../components/Model3DViewer";
import SearchableList from "../components/SearchableList";

interface Props {
  lang: Lang;
}

/** Which Showroom zone goes under which inventory category — weapons/shields/misc-weapons all
 * read as one "Weapons" shelf here (the Showroom keeps them on separate walls/tables for the
 * physical layout, but browsing by inventory slot doesn't need that split). */
type Category = "weapons" | "potions" | "food" | "valuables" | "tools";
const ZONE_TO_CATEGORY: Record<ItemZoneId, Category> = {
  swords: "weapons",
  shields: "weapons",
  weaponsMisc: "weapons",
  potions: "potions",
  food: "food",
  valuables: "valuables",
  tools: "tools",
};
const CATEGORIES: Category[] = ["weapons", "potions", "food", "valuables", "tools"];
const CATEGORY_LABEL: Record<Category, { uk: string; en: string }> = {
  weapons: { uk: "⚔ Зброя", en: "⚔ Weapons" },
  potions: { uk: "🧪 Зілля", en: "🧪 Potions" },
  food: { uk: "🍞 Їжа", en: "🍞 Food" },
  valuables: { uk: "💎 Цінності", en: "💎 Valuables" },
  tools: { uk: "🔧 Інструменти", en: "🔧 Tools" },
};

const MODES: ViewMode[] = ["textured", "wireframe", "clay"];
const MODE_LABEL: Record<ViewMode, { uk: string; en: string }> = {
  textured: { uk: "Текстуровано", en: "Textured" },
  wireframe: { uk: "Каркас", en: "Wireframe" },
  clay: { uk: "Глина", en: "Clay" },
  normalMap: { uk: "Рельєф", en: "Relief" },
};

/** Real inventory items only (whatever `categorizeMesh` — the same curated list the Showroom
 * uses — assigns a zone to), grouped by inventory slot rather than by raw archive folder like
 * Models does. Pure browsing for now: no texture regeneration here (that's Models' job) — this
 * is groundwork for the owner's bigger idea (click an item → the hero performs an NPC-style
 * routine), which real research (2026-07-21) found needs a lot more reverse-engineering of the
 * game's compiled script/dialogue layer (`data/compiled/library.pak`'s `.xinf`/`.xqst` catalogs)
 * before it's buildable — not started, see memory. */
export default function Inventory({ lang }: Props) {
  const [meshes, setMeshes] = useState<MeshEntry[]>([]);
  const [textures, setTextures] = useState<LibraryEntry[]>([]);
  const [motions, setMotions] = useState<MotionEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  const [category, setCategory] = useState<Category>("weapons");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<MeshEntry | null>(null);
  const [objUrl, setObjUrl] = useState<string | null>(null);
  const [objError, setObjError] = useState<string | null>(null);
  const [objLoading, setObjLoading] = useState(false);
  const [mode, setMode] = useState<ViewMode>("textured");

  useEffect(() => {
    listMeshes()
      .then(setMeshes)
      .catch((e) => setError(String(e)));
    listLibrary()
      .then(setTextures)
      .catch(() => {});
    // Same real-body-clip filter Animations.tsx uses (archiveStem "animations", not the
    // per-line speech/lip-sync archives) — feeds `deriveScenarios` below.
    listMotions()
      .then((all) => setMotions(all.filter((m) => m.archiveStem === "animations")))
      .catch(() => {});
  }, []);

  // Owner request (2026-07-21): "додай сценарії до кожного ітема, щоб читати що закодовано" —
  // which of the Hero's real scenarios (see lib/scenarios.ts) this item's own name plausibly
  // encodes. Most items (weapons, tools) have none — a real, informative answer, not a bug.
  const scenarios = useMemo(() => deriveScenarios(motions), [motions]);
  const matchedScenarios = useMemo(() => (selected ? matchScenariosForItemName(scenarios, selected.name) : []), [scenarios, selected]);

  const itemsByCategory = useMemo(() => {
    const out: Record<Category, MeshEntry[]> = { weapons: [], potions: [], food: [], valuables: [], tools: [] };
    for (const m of meshes) {
      const zone = categorizeMesh(m);
      if (zone) out[ZONE_TO_CATEGORY[zone]].push(m);
    }
    return out;
  }, [meshes]);

  const visible = useMemo(() => filterEntries(itemsByCategory[category], query), [itemsByCategory, category, query]);

  function selectCategory(c: Category) {
    setCategory(c);
    setQuery("");
    setSelected(null);
  }

  useEffect(() => {
    if (!selected) {
      setObjUrl(null);
      return;
    }
    let cancelled = false;
    setObjUrl(null);
    setObjError(null);
    setObjLoading(true);
    meshObjUrl(selected.archivePath, selected.entryPath)
      .then((url) => {
        if (!cancelled) setObjUrl(url);
      })
      .catch((e) => {
        if (!cancelled) setObjError(String(e));
      })
      .finally(() => {
        if (!cancelled) setObjLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selected]);

  // Per-material auto-resolve only (no manual texture picker here, unlike Models) — same
  // lookup Showroom/Models both use.
  const resolveTexture = useCallback(
    async (baseName: string) => {
      const entry = findTextureEntryForBaseName(textures, baseName);
      return entry ? readTextureDataUrl(entry.pngRel) : null;
    },
    [textures],
  );

  return (
    <div style={{ flex: 1, display: "flex", minHeight: 0, position: "relative" }}>
      <div
        style={{
          width: 236,
          flexShrink: 0,
          background: "var(--bg1)",
          borderRight: "1px solid var(--border)",
          display: "flex",
          flexDirection: "column",
          minHeight: 0,
          padding: "14px 10px",
        }}
      >
        <div style={{ font: "600 10px system-ui", letterSpacing: ".06em", textTransform: "uppercase", color: "var(--text-faint)", padding: "0 2px 8px" }}>
          {lang === "uk" ? "Інвентар" : "Inventory"}
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 4, marginBottom: 10 }}>
          {CATEGORIES.map((c) => (
            <button
              key={c}
              onClick={() => selectCategory(c)}
              style={{
                textAlign: "left",
                padding: "8px 10px",
                borderRadius: 8,
                background: category === c ? "var(--accent-tint)" : "transparent",
                border: "none",
                font: "600 12px system-ui",
                color: category === c ? "var(--text)" : "var(--text-dim)",
                display: "flex",
                justifyContent: "space-between",
              }}
            >
              <span>{CATEGORY_LABEL[c][lang]}</span>
              <span style={{ color: "var(--text-faint)", fontWeight: 500 }}>{itemsByCategory[c].length}</span>
            </button>
          ))}
        </div>
        <SearchableList
          items={visible}
          selectedName={selected?.name ?? null}
          onSelect={setSelected}
          query={query}
          onQueryChange={setQuery}
          placeholder={lang === "uk" ? "Пошук предмета…" : "Search items…"}
          limit={150}
        />
      </div>

      <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, minHeight: 0 }}>
        <div style={{ padding: "14px 20px", borderBottom: "1px solid var(--border)", display: "flex", alignItems: "center", gap: 14 }}>
          <div>
            <div style={{ font: "700 15px system-ui" }}>{selected?.name ?? (lang === "uk" ? "Оберіть предмет" : "Select an item")}</div>
            <div style={{ font: "500 11px system-ui", color: "var(--text-faint)" }}>
              {lang === "uk"
                ? "Реальні предмети інвентаря гри, згруповані по слоту — перегляд, поки без дій."
                : "Real in-game inventory items, grouped by slot — browsing only for now."}
            </div>
            {selected ? (
              <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 6, flexWrap: "wrap" }}>
                <span style={{ font: "600 10px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: "var(--text-faint)" }}>
                  {lang === "uk" ? "Закодовані сценарії:" : "Encoded scenarios:"}
                </span>
                {matchedScenarios.length > 0 ? (
                  matchedScenarios.map((s) => (
                    <span
                      key={s.id}
                      title={lang === "uk" ? "Переглянути живо: вкладка «Анімації» → цей же напис у списку" : "Watch it live: Animations tab → this same label in the list"}
                      style={{ padding: "3px 9px", borderRadius: 10, background: "var(--accent-tint)", font: "600 11px system-ui", color: "var(--text)" }}
                    >
                      {s.label}
                    </span>
                  ))
                ) : (
                  <span style={{ font: "500 11px system-ui", color: "var(--text-faint)" }}>
                    {lang === "uk" ? "немає (цей предмет не бере участі в жодному відомому сценарії)" : "none (this item isn't part of any known scenario)"}
                  </span>
                )}
              </div>
            ) : null}
          </div>
          <div style={{ flex: 1 }} />
          <div style={{ display: "flex", gap: 6 }}>
            {MODES.map((m) => (
              <button
                key={m}
                onClick={() => setMode(m)}
                style={{
                  padding: "7px 14px",
                  borderRadius: 9,
                  background: mode === m ? "var(--accent)" : "var(--bg2)",
                  border: `1px solid ${mode === m ? "var(--accent)" : "var(--border)"}`,
                  font: "600 11.5px system-ui",
                  color: mode === m ? "#fff" : "var(--text-dim)",
                  whiteSpace: "nowrap",
                }}
              >
                {MODE_LABEL[m][lang]}
              </button>
            ))}
          </div>
        </div>
        <div style={{ flex: 1, minHeight: 0, position: "relative" }}>
          {error ? (
            <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--red)" }}>{error}</div>
          ) : !selected ? (
            <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
              {lang === "uk" ? "Оберіть предмет зліва" : "Select an item on the left"}
            </div>
          ) : objError ? (
            <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--red)" }}>{objError}</div>
          ) : objLoading || !objUrl ? (
            <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
              {lang === "uk" ? "Конвертація мешу…" : "Converting mesh…"}
            </div>
          ) : (
            <Model3DViewer key={selected.entryPath} objUrl={objUrl} diffuseUrl={null} normalUrl={null} mode={mode} resolveTexture={resolveTexture} />
          )}
        </div>
      </div>
    </div>
  );
}
