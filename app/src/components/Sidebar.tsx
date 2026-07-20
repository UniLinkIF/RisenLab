import type { ReactNode } from "react";
import type { Lang } from "../lib/i18n";
import { t } from "../lib/i18n";

export type Screen = "dashboard" | "library" | "models" | "animations" | "showroom" | "inventory" | "guide" | "settings";

interface Props {
  active: Screen;
  onNavigate: (screen: Screen) => void;
  lang: Lang;
}

function RailButton({
  active,
  disabled,
  title,
  onClick,
  children,
}: {
  active?: boolean;
  disabled?: boolean;
  title?: string;
  onClick?: () => void;
  children: ReactNode;
}) {
  return (
    <button
      title={title}
      onClick={disabled ? undefined : onClick}
      style={{
        width: 40,
        height: 40,
        borderRadius: 11,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: active ? "var(--accent-tint)" : "transparent",
        border: "none",
        opacity: disabled ? 0.35 : 1,
        cursor: disabled ? "default" : "pointer",
      }}
    >
      {children}
    </button>
  );
}

// Same icon rail slots as the approved design (LibraryScreen/SettingsScreen .dc.html):
// Dashboard/Library/Models/Settings switch the main screen. There is deliberately no separate
// Search rail slot — Library's own search box is the one true search (see
// [[risenlab-ui-vision]]: a dedicated search screen was a near-duplicate of Library's own
// grid+filter). The top square slot (originally an undefined placeholder) is now Dashboard,
// per the owner's request for an overview tab.
export default function Sidebar({ active, onNavigate, lang }: Props) {
  return (
    <div
      style={{
        width: 64,
        flexShrink: 0,
        background: "var(--bg1)",
        borderRight: "1px solid var(--border)",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 6,
        padding: "14px 0",
      }}
    >
      <RailButton active={active === "dashboard"} title={t(lang).navDashboard} onClick={() => onNavigate("dashboard")}>
        <div
          style={{
            width: 16,
            height: 16,
            borderRadius: 4,
            border: `1.5px solid ${active === "dashboard" ? "var(--accent)" : "var(--text-faint)"}`,
          }}
        />
      </RailButton>

      <RailButton active={active === "library"} title={t(lang).navLibrary} onClick={() => onNavigate("library")}>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "6px 6px",
            gridTemplateRows: "6px 6px",
            gap: 2,
          }}
        >
          {[0, 1, 2, 3].map((i) => (
            <div
              key={i}
              style={{ background: active === "library" ? "var(--accent)" : "var(--text-faint)", borderRadius: 1 }}
            />
          ))}
        </div>
      </RailButton>

      <RailButton active={active === "models"} title={t(lang).navModels} onClick={() => onNavigate("models")}>
        <div
          style={{
            width: 0,
            height: 0,
            borderTop: "6px solid transparent",
            borderBottom: "6px solid transparent",
            borderLeft: `9px solid ${active === "models" ? "var(--accent)" : "var(--text-faint)"}`,
          }}
        />
      </RailButton>

      <RailButton active={active === "animations"} title={t(lang).navAnimations} onClick={() => onNavigate("animations")}>
        <div
          style={{
            width: 16,
            height: 16,
            borderRadius: "50%",
            border: `1.5px solid ${active === "animations" ? "var(--accent)" : "var(--text-faint)"}`,
            position: "relative",
          }}
        >
          <div
            style={{
              position: "absolute",
              top: 2,
              left: 6,
              width: 2,
              height: 5,
              background: active === "animations" ? "var(--accent)" : "var(--text-faint)",
              transformOrigin: "bottom center",
              transform: "rotate(35deg)",
            }}
          />
        </div>
      </RailButton>

      <RailButton active={active === "showroom"} title={t(lang).navShowroom} onClick={() => onNavigate("showroom")}>
        {/* A little "pedestal with an item on it" glyph — distinct from Models' plain triangle. */}
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 2 }}>
          <div style={{ width: 7, height: 7, borderRadius: "50%", background: active === "showroom" ? "var(--accent)" : "var(--text-faint)" }} />
          <div style={{ width: 16, height: 3, borderRadius: 1, background: active === "showroom" ? "var(--accent)" : "var(--text-faint)" }} />
          <div style={{ width: 10, height: 2, borderRadius: 1, background: active === "showroom" ? "var(--accent)" : "var(--text-faint)" }} />
        </div>
      </RailButton>

      <RailButton active={active === "inventory"} title={t(lang).navInventory} onClick={() => onNavigate("inventory")}>
        {/* A little bag/pouch glyph — distinct from Showroom's pedestal. */}
        <div
          style={{
            width: 13,
            height: 11,
            borderRadius: "3px 3px 6px 6px",
            border: `1.5px solid ${active === "inventory" ? "var(--accent)" : "var(--text-faint)"}`,
            position: "relative",
          }}
        >
          <div
            style={{
              position: "absolute",
              top: -4,
              left: "50%",
              transform: "translateX(-50%)",
              width: 7,
              height: 5,
              borderRadius: "4px 4px 0 0",
              border: `1.5px solid ${active === "inventory" ? "var(--accent)" : "var(--text-faint)"}`,
              borderBottom: "none",
            }}
          />
        </div>
      </RailButton>

      <div style={{ flex: 1 }} />

      <RailButton active={active === "guide"} title={t(lang).navGuide} onClick={() => onNavigate("guide")}>
        <div
          style={{
            width: 16,
            height: 16,
            borderRadius: "50%",
            border: `1.5px solid ${active === "guide" ? "var(--accent)" : "var(--text-faint)"}`,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            font: `700 11px system-ui`,
            color: active === "guide" ? "var(--accent)" : "var(--text-faint)",
          }}
        >
          ?
        </div>
      </RailButton>

      <RailButton active={active === "settings"} title={t(lang).navSettings} onClick={() => onNavigate("settings")}>
        <div
          style={{
            width: 14,
            height: 14,
            borderRadius: "50%",
            border: `1.5px solid ${active === "settings" ? "var(--accent)" : "var(--text-faint)"}`,
          }}
        />
      </RailButton>
    </div>
  );
}
