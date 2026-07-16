import { useEffect, useState } from "react";
import type { Lang } from "./lib/i18n";
import type { AppSettings, LibraryEntry } from "./lib/types";
import { getSettings, saveSettings } from "./lib/api";
import Titlebar from "./components/Titlebar";
import Sidebar, { type Screen } from "./components/Sidebar";
import Dashboard from "./screens/Dashboard";
import Library from "./screens/Library";
import Models from "./screens/Models";
import Animations from "./screens/Animations";
import AiCompare from "./screens/AiCompare";
import Settings from "./screens/Settings";

export default function App() {
  const [lang, setLang] = useState<Lang>("uk");
  const [screen, setScreen] = useState<Screen | "ai-compare">("dashboard");
  const [aiPngRel, setAiPngRel] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);

  useEffect(() => {
    getSettings().then((s) => {
      setLang(s.language);
      setConnected(!!s.gameExe);
    });
  }, []);

  async function changeLang(l: Lang) {
    setLang(l);
    const current = await getSettings();
    await saveSettings({ ...current, language: l });
  }

  function handleRegenerated(entry: LibraryEntry) {
    setAiPngRel(entry.pngRel);
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
      />
      <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
        <Sidebar active={screen === "ai-compare" ? "library" : screen} onNavigate={(s) => setScreen(s)} lang={lang} />
        {screen === "dashboard" ? <Dashboard lang={lang} /> : null}
        {screen === "library" ? <Library lang={lang} onRegenerated={handleRegenerated} /> : null}
        {screen === "models" ? <Models lang={lang} /> : null}
        {screen === "animations" ? <Animations lang={lang} /> : null}
        {screen === "settings" ? (
          <Settings lang={lang} onLangChange={changeLang} onSettingsSaved={handleSettingsSaved} />
        ) : null}
        {screen === "ai-compare" ? <AiCompare lang={lang} initialPngRel={aiPngRel} /> : null}
      </div>
    </div>
  );
}
