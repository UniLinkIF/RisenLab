import { useCallback, useEffect, useState } from "react";
import type { Lang } from "./lib/i18n";
import type { AppSettings, LibraryEntry } from "./lib/types";
import { getSettings, saveSettings, reviewQueue } from "./lib/api";
import Titlebar from "./components/Titlebar";
import Sidebar, { type Screen } from "./components/Sidebar";
import Dashboard from "./screens/Dashboard";
import Library from "./screens/Library";
import Models from "./screens/Models";
import Animations from "./screens/Animations";
import Showroom from "./screens/Showroom";
import AiCompare from "./screens/AiCompare";
import ErrorLog from "./components/ErrorLog";
import Settings from "./screens/Settings";
import Guide from "./screens/Guide";

export default function App() {
  const [lang, setLang] = useState<Lang>("uk");
  const [screen, setScreen] = useState<Screen | "ai-compare">("dashboard");
  const [aiPngRel, setAiPngRel] = useState<string | null>(null);
  // 3D context for the review screen: set when the review was opened from Models (we know
  // which mesh the texture was generated for), null when opened from the Library.
  const [aiModelObj, setAiModelObj] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);
  // Live "how many textures are waiting for a decision" count, shown as a persistent Titlebar
  // button — this is the owner's fix for "I batch-generated 1000 textures, then went to
  // Models and generated one, and it yanked me into approving the whole queue": no screen may
  // force-navigate into review anymore (see Library.tsx/Models.tsx), so the ONLY way in is
  // this button, clicked when the user actually wants to review, from wherever they are.
  const [pendingReviewCount, setPendingReviewCount] = useState(0);

  const refreshPendingReview = useCallback(() => {
    reviewQueue()
      .then((items) => setPendingReviewCount(items.filter((i) => i.status === "pending").length))
      .catch(() => {});
  }, []);

  useEffect(() => {
    getSettings().then((s) => {
      setLang(s.language);
      setConnected(!!s.gameExe);
    });
  }, []);

  // Re-syncs the count on mount and on every screen change — cheaply catches
  // approvals/rejections made in ai-compare (once the user leaves it) without polling on a timer.
  useEffect(() => {
    refreshPendingReview();
  }, [screen, refreshPendingReview]);

  // Cheap optimistic bump for the hot path (a running 1000-texture batch): no CLI round-trip,
  // just increments the badge immediately; the screen-change effect above corrects any drift.
  const bumpPendingReview = useCallback((delta: number) => {
    setPendingReviewCount((n) => Math.max(0, n + delta));
  }, []);

  function handleOpenReviewQueue() {
    setAiPngRel(null);
    setAiModelObj(null);
    setScreen("ai-compare");
  }

  async function changeLang(l: Lang) {
    setLang(l);
    const current = await getSettings();
    await saveSettings({ ...current, language: l });
  }

  function handleRegenerated(entry: LibraryEntry, modelObjUrl?: string | null) {
    setAiPngRel(entry.pngRel);
    setAiModelObj(modelObjUrl ?? null);
    setScreen("ai-compare");
  }

  function handleSettingsSaved(settings: AppSettings) {
    setConnected(!!settings.gameExe);
  }

  return (
    <div style={{ height: "100vh", display: "flex", flexDirection: "column", borderRadius: 14, overflow: "hidden" }}>
      <Titlebar
        lang={lang}
        onLangChange={changeLang}
        connected={screen !== "ai-compare" ? connected : undefined}
        pendingReviewCount={pendingReviewCount}
        onOpenReview={screen !== "ai-compare" ? handleOpenReviewQueue : undefined}
      />
      <div className="screen-host" style={{ flex: 1, display: "flex", minHeight: 0 }}>
        <Sidebar active={screen === "ai-compare" ? "library" : screen} onNavigate={(s) => setScreen(s)} lang={lang} />
        {screen === "dashboard" ? <Dashboard lang={lang} /> : null}
        {screen === "library" ? (
          <Library lang={lang} onRegenerated={handleRegenerated} onQueueChanged={() => bumpPendingReview(1)} onOpenReviewQueue={handleOpenReviewQueue} />
        ) : null}
        {screen === "models" ? <Models lang={lang} onRegenerated={handleRegenerated} onQueueChanged={() => bumpPendingReview(1)} /> : null}
        {screen === "animations" ? <Animations lang={lang} /> : null}
        {screen === "showroom" ? <Showroom lang={lang} /> : null}
        {screen === "guide" ? <Guide lang={lang} /> : null}
        {screen === "settings" ? (
          <Settings lang={lang} onLangChange={changeLang} onSettingsSaved={handleSettingsSaved} />
        ) : null}
        {screen === "ai-compare" ? <AiCompare lang={lang} initialPngRel={aiPngRel} modelObjUrl={aiModelObj} /> : null}
      </div>
      <ErrorLog lang={lang} />
    </div>
  );
}
