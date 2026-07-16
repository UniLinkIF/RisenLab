import type { ReactNode } from "react";
import type { Lang } from "../lib/i18n";
import { t } from "../lib/i18n";
import { closeWindow, minimizeWindow, toggleMaximizeWindow } from "../lib/windowControls";

interface Props {
  lang: Lang;
  onLangChange: (l: Lang) => void;
  connected?: boolean;
  centerLabel?: string;
}

function WinButton({ onClick, hoverColor, children }: { onClick: () => void; hoverColor?: string; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      onMouseEnter={(e) => (e.currentTarget.style.background = hoverColor ?? "var(--bg2)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
      style={{
        width: 32,
        height: 28,
        border: "none",
        background: "transparent",
        color: "var(--text-faint)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        borderRadius: 6,
        font: "600 13px system-ui",
      }}
    >
      {children}
    </button>
  );
}

// The window is borderless (decorations:false) so we draw our own chrome — a small logo
// mark instead of decorative (non-functional on Windows) macOS traffic lights, plus real
// working minimize/maximize/close controls since we removed the native ones.
export default function Titlebar({ lang, onLangChange, connected, centerLabel }: Props) {
  const s = t(lang);
  return (
    <div
      data-tauri-drag-region
      style={{
        height: 44,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        gap: 14,
        padding: "0 8px 0 16px",
        background: "var(--bg1)",
        borderBottom: "1px solid var(--border)",
      }}
    >
      <div
        style={{
          width: 22,
          height: 22,
          borderRadius: 7,
          background: "linear-gradient(135deg, var(--accent), oklch(0.5 0.19 300))",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          font: "700 12px system-ui",
          color: "#fff",
          flexShrink: 0,
        }}
      >
        R
      </div>
      <div style={{ font: "600 13px system-ui", color: "var(--text-dim)" }}>RisenLab</div>
      {centerLabel ? (
        <>
          <div style={{ flex: 1 }} />
          <div style={{ font: "600 12.5px system-ui", color: "var(--text-dim)" }}>{centerLabel}</div>
        </>
      ) : null}
      <div style={{ flex: 1 }} />
      {connected !== undefined ? (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            font: "500 12px system-ui",
            color: "var(--text-dim)",
            background: "var(--bg2)",
            border: "1px solid var(--border)",
            padding: "6px 12px",
            borderRadius: 20,
          }}
        >
          <div
            style={{
              width: 7,
              height: 7,
              borderRadius: "50%",
              background: connected ? "var(--green)" : "var(--text-faint)",
            }}
          />
          {connected ? s.connected : s.disconnected}
        </div>
      ) : null}
      <div
        style={{
          display: "flex",
          background: "var(--bg2)",
          border: "1px solid var(--border)",
          borderRadius: 20,
          padding: 3,
          gap: 2,
        }}
      >
        {(["uk", "en"] as Lang[]).map((l) => (
          <button
            key={l}
            onClick={() => onLangChange(l)}
            style={{
              padding: "5px 12px",
              borderRadius: 16,
              border: "none",
              font: "600 11px system-ui",
              background: lang === l ? "var(--accent)" : "transparent",
              color: lang === l ? "#fff" : "var(--text-faint)",
            }}
          >
            {l.toUpperCase()}
          </button>
        ))}
      </div>
      <div style={{ display: "flex", gap: 2, marginLeft: 4 }}>
        <WinButton onClick={minimizeWindow}>–</WinButton>
        <WinButton onClick={toggleMaximizeWindow}>□</WinButton>
        <WinButton onClick={closeWindow} hoverColor="#e81123">
          ×
        </WinButton>
      </div>
    </div>
  );
}
