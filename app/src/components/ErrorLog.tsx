import { useEffect, useState } from "react";
import type { Lang } from "../lib/i18n";

/** One captured error with enough context to report it ("круте логування помилок"). */
interface LoggedError {
  time: string;
  message: string;
  detail?: string;
}

const MAX_ERRORS = 50;

/** Global error capture: window errors + unhandled promise rejections (every failed API call
 * that a screen forgot to surface ends up here too). A floating badge appears bottom-left
 * only when something was actually caught; the panel shows the log with one-click copy for
 * pasting into a bug report. */
export default function ErrorLog({ lang }: { lang: Lang }) {
  const uk = lang === "uk";
  const [errors, setErrors] = useState<LoggedError[]>([]);
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const push = (message: string, detail?: string) =>
      setErrors((prev) =>
        [{ time: new Date().toLocaleTimeString(), message, detail }, ...prev].slice(0, MAX_ERRORS),
      );
    const onError = (e: ErrorEvent) => push(e.message || "Unknown error", e.error?.stack);
    const onRejection = (e: PromiseRejectionEvent) => {
      const r = e.reason;
      push(r instanceof Error ? r.message : String(r), r instanceof Error ? r.stack : undefined);
    };
    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);
    return () => {
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
    };
  }, []);

  if (errors.length === 0) return null;

  async function copyAll() {
    const text = errors.map((e) => `[${e.time}] ${e.message}${e.detail ? `\n${e.detail}` : ""}`).join("\n\n");
    await navigator.clipboard.writeText(text).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  return (
    <>
      <button
        onClick={() => setOpen((v) => !v)}
        title={uk ? "Журнал помилок" : "Error log"}
        style={{
          position: "fixed",
          bottom: 14,
          left: 14,
          zIndex: 90,
          padding: "6px 12px",
          borderRadius: 16,
          background: "var(--bg1)",
          border: "1px solid var(--red)",
          font: "700 11px system-ui",
          color: "var(--red)",
          boxShadow: "0 2px 10px rgba(0,0,0,.35)",
        }}
      >
        ⚠ {errors.length}
      </button>
      {open ? (
        <div
          style={{
            position: "fixed",
            bottom: 52,
            left: 14,
            zIndex: 90,
            width: 480,
            maxWidth: "calc(100vw - 28px)",
            maxHeight: "50vh",
            display: "flex",
            flexDirection: "column",
            background: "var(--bg1)",
            border: "1px solid var(--border-strong)",
            borderRadius: 12,
            boxShadow: "0 8px 30px rgba(0,0,0,.5)",
            overflow: "hidden",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 14px", borderBottom: "1px solid var(--border)" }}>
            <div style={{ font: "700 12px system-ui", color: "var(--text)", flex: 1 }}>
              {uk ? "Журнал помилок" : "Error log"}
            </div>
            <button onClick={copyAll} style={{ padding: "4px 10px", borderRadius: 8, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 11px system-ui", color: "var(--text-dim)" }}>
              {copied ? (uk ? "Скопійовано ✓" : "Copied ✓") : uk ? "Копіювати все" : "Copy all"}
            </button>
            <button onClick={() => setErrors([])} style={{ padding: "4px 10px", borderRadius: 8, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 11px system-ui", color: "var(--text-dim)" }}>
              {uk ? "Очистити" : "Clear"}
            </button>
          </div>
          <div style={{ overflow: "auto", padding: "8px 14px" }}>
            {errors.map((e, i) => (
              <div key={i} style={{ padding: "7px 0", borderBottom: i < errors.length - 1 ? "1px solid var(--border)" : "none" }}>
                <div style={{ font: "600 12px system-ui", color: "var(--red)" }}>
                  <span style={{ color: "var(--text-faint)", fontWeight: 500 }}>[{e.time}]</span> {e.message}
                </div>
                {e.detail ? (
                  <div style={{ font: "500 10.5px ui-monospace, Menlo, monospace", color: "var(--text-faint)", whiteSpace: "pre-wrap", marginTop: 3, maxHeight: 90, overflow: "auto" }}>
                    {e.detail}
                  </div>
                ) : null}
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </>
  );
}
