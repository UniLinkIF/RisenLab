import { useCallback, useEffect, useMemo, useState } from "react";
import type { Lang } from "../lib/i18n";
import type { LibraryEntry, MeshEntry } from "../lib/types";
import { listLibrary, listMeshes, meshObjUrl, meshTextureRefs, readEditedDataUrl, readTextureDataUrl, regenerateTexture } from "../lib/api";
import { buildFolderTree, filterByTreeKey, filterEntries, findTextureByBaseName } from "../lib/library";
import { findTextureEntryForBaseName } from "../lib/materials";
import FolderTree from "../components/FolderTree";
import Model3DViewer, { type ViewMode } from "../components/Model3DViewer";
import SearchableList from "../components/SearchableList";

interface Props {
  lang: Lang;
}

const MODES: ViewMode[] = ["textured", "wireframe", "clay", "normalMap"];
const MODE_LABEL: Record<ViewMode, { uk: string; en: string }> = {
  textured: { uk: "Текстуровано", en: "Textured" },
  wireframe: { uk: "Каркас", en: "Wireframe" },
  clay: { uk: "Глина", en: "Clay" },
  normalMap: { uk: "Рельєф (normal map)", en: "Relief (normal map)" },
};

export default function Models({ lang }: Props) {
  const [meshes, setMeshes] = useState<MeshEntry[]>([]);
  const [textures, setTextures] = useState<LibraryEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  const [meshTreeKey, setMeshTreeKey] = useState<string | null>(null);
  const [meshQuery, setMeshQuery] = useState("");
  const [selectedMesh, setSelectedMesh] = useState<MeshEntry | null>(null);
  const [objUrl, setObjUrl] = useState<string | null>(null);
  const [objError, setObjError] = useState<string | null>(null);
  const [objLoading, setObjLoading] = useState(false);

  const [mode, setMode] = useState<ViewMode>("textured");

  const [texturePicker, setTexturePicker] = useState<"diffuse" | "normal" | null>(null);
  const [textureQuery, setTextureQuery] = useState("");
  const [diffuseEntry, setDiffuseEntry] = useState<LibraryEntry | null>(null);
  const [normalEntry, setNormalEntry] = useState<LibraryEntry | null>(null);
  const [diffuseUrl, setDiffuseUrl] = useState<string | null>(null);
  const [normalUrl, setNormalUrl] = useState<string | null>(null);
  const [showingGenerated, setShowingGenerated] = useState(false);
  const [generating, setGenerating] = useState(false);
  // True after the user explicitly picks a texture by hand — per-material auto-resolution is
  // then suspended so the explicit choice actually shows on the whole mesh instead of being
  // overridden per submesh. Reset whenever a new mesh's auto-match runs.
  const [manualTexture, setManualTexture] = useState(false);

  useEffect(() => {
    listMeshes()
      .then(setMeshes)
      .catch((e) => setError(String(e)));
    listLibrary()
      .then(setTextures)
      .catch(() => {});
  }, []);

  const meshTree = useMemo(() => buildFolderTree(meshes), [meshes]);
  const visibleMeshes = useMemo(
    () => filterEntries(filterByTreeKey(meshes, meshTreeKey), meshQuery),
    [meshes, meshTreeKey, meshQuery],
  );
  const visibleTextures = useMemo(() => filterEntries(textures, textureQuery), [textures, textureQuery]);

  useEffect(() => {
    if (!selectedMesh) {
      setObjUrl(null);
      return;
    }
    let cancelled = false;
    setObjUrl(null);
    setObjError(null);
    setObjLoading(true);
    meshObjUrl(selectedMesh.archivePath, selectedMesh.entryPath)
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
  }, [selectedMesh]);

  // Auto-match: the mesh's own material already references real texture file names (see
  // MaterialTextureRefs) — use those instead of making the user hunt for the right texture by
  // hand. Only overrides the picker when a real match is found in the library; an unmatched
  // slot is left as "none" rather than keeping a stale texture from a previously selected mesh.
  useEffect(() => {
    if (!selectedMesh || textures.length === 0) return;
    let cancelled = false;
    meshTextureRefs(selectedMesh.archivePath, selectedMesh.entryPath)
      .then((refs) => {
        if (cancelled) return;
        setDiffuseEntry(refs.diffuse ? findTextureByBaseName(textures, refs.diffuse) : null);
        setNormalEntry(refs.normal ? findTextureByBaseName(textures, refs.normal) : null);
        setShowingGenerated(false);
        setManualTexture(false);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [selectedMesh, textures]);

  useEffect(() => {
    if (!diffuseEntry) {
      setDiffuseUrl(null);
      return;
    }
    let cancelled = false;
    (showingGenerated ? readEditedDataUrl : readTextureDataUrl)(diffuseEntry.pngRel).then((url) => {
      if (!cancelled) setDiffuseUrl(url);
    });
    return () => {
      cancelled = true;
    };
  }, [diffuseEntry, showingGenerated]);

  useEffect(() => {
    if (!normalEntry) {
      setNormalUrl(null);
      return;
    }
    let cancelled = false;
    readTextureDataUrl(normalEntry.pngRel).then((url) => {
      if (!cancelled) setNormalUrl(url);
    });
    return () => {
      cancelled = true;
    };
  }, [normalEntry]);

  function pickTexture(entry: LibraryEntry) {
    if (texturePicker === "diffuse") {
      setDiffuseEntry(entry);
      setShowingGenerated(false);
    } else if (texturePicker === "normal") {
      setNormalEntry(entry);
    }
    setManualTexture(true);
    setTexturePicker(null);
    setTextureQuery("");
  }

  // Per-material texture resolution for multi-material meshes (see the matching prop doc in
  // Model3DViewer): a material's `usemtl` name is its diffuse texture's base name in this
  // game's real data, so the library lookup is the same one the auto-match uses.
  const resolveTexture = useCallback(
    async (baseName: string) => {
      const entry = findTextureEntryForBaseName(textures, baseName);
      return entry ? readTextureDataUrl(entry.pngRel) : null;
    },
    [textures],
  );

  async function handleGenerate() {
    if (!diffuseEntry) return;
    setGenerating(true);
    try {
      await regenerateTexture(diffuseEntry.pngRel);
      setShowingGenerated(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setGenerating(false);
    }
  }

  return (
    <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
      <FolderTree nodes={meshTree} selectedKey={meshTreeKey} onSelect={setMeshTreeKey} title={lang === "uk" ? "Архіви" : "Archives"} />

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
        <div
          style={{
            font: "600 10px system-ui",
            letterSpacing: ".06em",
            textTransform: "uppercase",
            color: "var(--text-faint)",
            padding: "0 2px 8px",
          }}
        >
          {lang === "uk" ? `Моделі (${meshes.length} реальних)` : `Models (${meshes.length} real)`}
        </div>
        <SearchableList
          items={visibleMeshes}
          selectedName={selectedMesh?.name ?? null}
          onSelect={setSelectedMesh}
          query={meshQuery}
          onQueryChange={setMeshQuery}
          placeholder={lang === "uk" ? "Пошук моделі…" : "Search models…"}
          limit={150}
        />
      </div>

      <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, minHeight: 0 }}>
        <div style={{ display: "flex", gap: 8, padding: "16px 20px", borderBottom: "1px solid var(--border)" }}>
          {MODES.map((m) => (
            <button
              key={m}
              onClick={() => setMode(m)}
              style={{
                padding: "7px 14px",
                borderRadius: 16,
                background: mode === m ? "var(--accent)" : "var(--bg2)",
                border: `1px solid ${mode === m ? "var(--accent)" : "var(--border)"}`,
                font: "600 11.5px system-ui",
                color: mode === m ? "#fff" : "var(--text-dim)",
              }}
            >
              {MODE_LABEL[m][lang]}
            </button>
          ))}
        </div>
        <div style={{ flex: 1, minHeight: 0, position: "relative" }}>
          {!selectedMesh ? (
            <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
              {lang === "uk" ? "Оберіть модель зліва" : "Select a model on the left"}
            </div>
          ) : objError ? (
            <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--red)" }}>
              {objError}
            </div>
          ) : objLoading || !objUrl ? (
            <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
              {lang === "uk" ? "Конвертація мешу…" : "Converting mesh…"}
            </div>
          ) : (
            <Model3DViewer
              key={selectedMesh.entryPath}
              objUrl={objUrl}
              diffuseUrl={diffuseUrl}
              normalUrl={normalUrl}
              mode={mode}
              // An explicit user pick (or the generated-texture preview) must win over
              // per-material auto-resolution, so it's suspended for those.
              resolveTexture={manualTexture || showingGenerated ? null : resolveTexture}
            />
          )}
        </div>
      </div>

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
        {error ? <div style={{ color: "var(--red)", marginBottom: 12, font: "500 12px system-ui" }}>{error}</div> : null}

        <div style={{ font: "700 13px system-ui", color: "var(--text)", marginBottom: 10 }}>
          {lang === "uk" ? "Текстура" : "Texture"}
        </div>

        {(["diffuse", "normal"] as const).map((slot) => {
          const entry = slot === "diffuse" ? diffuseEntry : normalEntry;
          const label = slot === "diffuse" ? (lang === "uk" ? "Дифузна" : "Diffuse") : lang === "uk" ? "Рельєф" : "Normal";
          return (
            <div key={slot} style={{ marginBottom: 14 }}>
              <div style={{ font: "600 10px system-ui", letterSpacing: ".04em", textTransform: "uppercase", color: "var(--text-faint)", marginBottom: 5 }}>
                {label}
              </div>
              <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <div
                  style={{
                    flex: 1,
                    font: "500 12px ui-monospace, Menlo, monospace",
                    color: entry ? "var(--text)" : "var(--text-faint)",
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    padding: "7px 9px",
                    background: "var(--bg2)",
                    borderRadius: 7,
                  }}
                >
                  {entry?.name ?? (lang === "uk" ? "не вибрано" : "none")}
                </div>
                <button
                  onClick={() => setTexturePicker(texturePicker === slot ? null : slot)}
                  style={{
                    padding: "7px 10px",
                    borderRadius: 7,
                    background: texturePicker === slot ? "var(--accent)" : "var(--bg2)",
                    border: "1px solid var(--border)",
                    font: "600 11.5px system-ui",
                    color: texturePicker === slot ? "#fff" : "var(--text)",
                  }}
                >
                  {lang === "uk" ? "Змінити" : "Change"}
                </button>
              </div>
              {texturePicker === slot ? (
                <div style={{ marginTop: 8, height: 220, display: "flex" }}>
                  <SearchableList
                    items={visibleTextures}
                    selectedName={entry?.name ?? null}
                    onSelect={pickTexture}
                    query={textureQuery}
                    onQueryChange={setTextureQuery}
                    placeholder={lang === "uk" ? "Пошук текстури…" : "Search textures…"}
                    limit={60}
                  />
                </div>
              ) : null}
            </div>
          );
        })}

        <button
          disabled={!diffuseEntry || generating}
          onClick={handleGenerate}
          style={{
            width: "100%",
            padding: 10,
            borderRadius: 9,
            background: "var(--accent)",
            border: "none",
            font: "600 12.5px system-ui",
            color: "#fff",
            opacity: !diffuseEntry || generating ? 0.5 : 1,
            marginTop: 4,
          }}
        >
          {generating ? (lang === "uk" ? "Генерація…" : "Generating…") : lang === "uk" ? "✨ Згенерувати нову текстуру" : "✨ Generate new texture"}
        </button>

        {diffuseEntry && showingGenerated ? (
          <button
            onClick={() => setShowingGenerated((v) => !v)}
            style={{
              width: "100%",
              padding: 9,
              borderRadius: 9,
              background: "var(--bg2)",
              border: "1px solid var(--border)",
              font: "600 12px system-ui",
              color: "var(--text)",
              marginTop: 8,
            }}
          >
            {showingGenerated
              ? lang === "uk"
                ? "Показати оригінал"
                : "Show original"
              : lang === "uk"
                ? "Показати згенеровану"
                : "Show generated"}
          </button>
        ) : null}
      </div>
    </div>
  );
}
