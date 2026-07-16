import type { TreeNode } from "../lib/library";

interface Props {
  nodes: TreeNode[];
  selectedKey: string | null;
  onSelect: (key: string | null) => void;
  title: string;
}

export default function FolderTree({ nodes, selectedKey, onSelect, title }: Props) {
  return (
    <div
      style={{
        width: 236,
        flexShrink: 0,
        background: "var(--bg1)",
        borderRight: "1px solid var(--border)",
        overflow: "auto",
        padding: "14px 8px",
      }}
    >
      <div
        style={{
          font: "600 10px system-ui",
          letterSpacing: ".06em",
          textTransform: "uppercase",
          color: "var(--text-faint)",
          padding: "4px 10px 8px",
        }}
      >
        {title}
      </div>
      <div
        onClick={() => onSelect(null)}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 7,
          padding: "6px 10px",
          borderRadius: 7,
          background: selectedKey === null ? "var(--accent-tint)" : "transparent",
          cursor: "pointer",
        }}
      >
        <div style={{ font: "500 12.5px system-ui", color: selectedKey === null ? "var(--text)" : "var(--text-dim)" }}>
          Всі / All
        </div>
      </div>
      {nodes.map((node) => (
        <div
          key={node.key}
          onClick={() => onSelect(node.key)}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 7,
            padding: "6px 10px",
            marginLeft: node.depth * 14,
            borderRadius: 7,
            background: selectedKey === node.key ? "var(--accent-tint)" : "transparent",
            cursor: "pointer",
          }}
        >
          <div
            style={{
              width: 9,
              height: 9,
              borderRadius: 2,
              background: selectedKey === node.key ? "var(--accent)" : "var(--text-faint)",
              flexShrink: 0,
            }}
          />
          <div
            style={{
              font: "500 12.5px system-ui",
              color: selectedKey === node.key ? "var(--text)" : "var(--text-dim)",
              flex: 1,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {node.label}
          </div>
          <div style={{ font: "500 11px ui-monospace, Menlo, monospace", color: "var(--text-faint)" }}>
            {node.count}
          </div>
        </div>
      ))}
    </div>
  );
}
