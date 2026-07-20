// Ported (and extended where the real app needs strings the design mockup didn't need,
// e.g. error/loading states, the Build Patch action) from the four approved .dc.html
// screens' own `STR.uk`/`STR.en` dictionaries — same wording, same keys where they overlap.

export type Lang = "uk" | "en";

export interface Strings {
  connected: string;
  disconnected: string;
  archives: string;
  libraryTitle: string;
  searchPlaceholder: string;
  previewPlaceholder: string;
  btnExtract: string;
  btnRegenerate: string;
  chipsAll: string;
  chipFolder: string;
  chipType: string;
  chipStatus: string;
  chipFmt: string;
  aiTitle: string;
  original: string;
  variant: string;
  approve: string;
  reject: string;
  regenerate: string;
  skip: string;
  settingsTitle: string;
  gameSection: string;
  gamePath: string;
  browse: string;
  check: string;
  found: string;
  textures: string;
  outputSection: string;
  appSection: string;
  language: string;
  buildPatch: string;
  loading: string;
  error: string;
  noSelection: string;
  soon: string;
  navDashboard: string;
  navLibrary: string;
  navModels: string;
  navAnimations: string;
  navShowroom: string;
  navGuide: string;
  navSettings: string;
}

const uk: Strings = {
  connected: "Risen.exe підключено",
  disconnected: "Гру не підключено",
  archives: "Архіви",
  libraryTitle: "Бібліотека текстур",
  searchPlaceholder: "Пошук за назвою…",
  previewPlaceholder: "ВЕЛИКЕ ПРЕВ'Ю\nТЕКСТУРИ",
  btnExtract: "Витягнути",
  btnRegenerate: "AI-регенерація",
  chipsAll: "Всі",
  chipFolder: "Папка",
  chipType: "Тип",
  chipStatus: "Статус",
  chipFmt: "Формат",
  aiTitle: "AI-редагування текстур",
  original: "Оригінал",
  variant: "AI-варіант",
  approve: "✓ Прийняти",
  reject: "✕ Відхилити",
  regenerate: "↻ Перегенерувати",
  skip: "→ Пропустити",
  settingsTitle: "Налаштування",
  gameSection: "Гра",
  gamePath: "Шлях до Risen.exe або ярлика (.lnk)",
  browse: "Огляд…",
  check: "Перевірити гру",
  found: "Знайдено, архіви завантажено",
  textures: "текстур",
  outputSection: "Папки виводу",
  appSection: "Застосунок",
  language: "Мова інтерфейсу",
  buildPatch: "Зібрати патч",
  loading: "Завантаження…",
  error: "Помилка",
  noSelection: "Оберіть текстуру",
  soon: "скоро",
  navDashboard: "Дашборд",
  navLibrary: "Бібліотека",
  navModels: "Моделі",
  navAnimations: "Анімації",
  navShowroom: "Вітрина",
  navGuide: "Інструкція",
  navSettings: "Налаштування",
};

const en: Strings = {
  connected: "Risen.exe connected",
  disconnected: "Game not connected",
  archives: "Archives",
  libraryTitle: "Texture Library",
  searchPlaceholder: "Search by name…",
  previewPlaceholder: "LARGE TEXTURE\nPREVIEW",
  btnExtract: "Extract",
  btnRegenerate: "AI Regenerate",
  chipsAll: "All",
  chipFolder: "Folder",
  chipType: "Type",
  chipStatus: "Status",
  chipFmt: "Format",
  aiTitle: "AI Texture Review",
  original: "Original",
  variant: "AI Variant",
  approve: "✓ Approve",
  reject: "✕ Reject",
  regenerate: "↻ Regenerate",
  skip: "→ Skip",
  settingsTitle: "Settings",
  gameSection: "Game",
  gamePath: "Path to Risen.exe or shortcut (.lnk)",
  browse: "Browse…",
  check: "Check Game",
  found: "Found, archives loaded",
  textures: "textures",
  outputSection: "Output Folders",
  appSection: "Application",
  language: "Interface Language",
  buildPatch: "Build Patch",
  loading: "Loading…",
  error: "Error",
  noSelection: "Select a texture",
  soon: "soon",
  navDashboard: "Dashboard",
  navLibrary: "Library",
  navModels: "Models",
  navAnimations: "Animations",
  navShowroom: "Showroom",
  navGuide: "Guide",
  navSettings: "Settings",
};

export const STR: Record<Lang, Strings> = { uk, en };

export function t(lang: Lang): Strings {
  return STR[lang];
}

export function queueCount(n: number, lang: Lang): string {
  return lang === "uk" ? `${n} текстур в черзі перевірки` : `${n} textures in review queue`;
}
