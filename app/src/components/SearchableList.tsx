/** A compact search box + scrollable name list — used for browsing meshes/actors/motions and
 * for picking a diffuse/normal texture from the real library, so this one component covers
 * every "find a real asset by name" spot across screens. */
export default function SearchableList<T extends { name: string }>({
  items,
  selectedName,
  onSelect,
  query,
  onQueryChange,
  placeholder,
  limit,
}: {
  items: T[];
  selectedName: string | null;
  onSelect: (item: T) => void;
  query: string;
  onQueryChange: (q: string) => void;
  placeholder: string;
  limit?: number;
}) {
  const shown = limit ? items.slice(0, limit) : items;
  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, flex: 1 }}>
      <input
        value={query}
        onChange={(e) => onQueryChange(e.target.value)}
        placeholder={placeholder}
        style={{
          background: "var(--bg2)",
          border: "1px solid var(--border)",
          borderRadius: 8,
          padding: "7px 10px",
          margin: "0 2px 8px",
          color: "var(--text)",
          font: "500 12px system-ui",
        }}
      />
      <div style={{ flex: 1, overflow: "auto" }}>
        {shown.map((item, i) => (
          <div
            key={`${item.name}-${i}`}
            onClick={() => onSelect(item)}
            style={{
              padding: "7px 10px",
              borderRadius: 7,
              cursor: "pointer",
              background: item.name === selectedName ? "var(--accent-tint)" : "transparent",
              font: "500 12px ui-monospace, Menlo, monospace",
              color: item.name === selectedName ? "var(--text)" : "var(--text-dim)",
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {item.name}
          </div>
        ))}
        {items.length > shown.length ? (
          <div style={{ padding: "6px 10px", font: "500 11px system-ui", color: "var(--text-faint)" }}>
            +{items.length - shown.length}…
          </div>
        ) : null}
      </div>
    </div>
  );
}
