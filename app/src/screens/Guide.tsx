import type { Lang } from "../lib/i18n";

interface Props {
  lang: Lang;
}

interface Section {
  title: string;
  body: string;
}

function SectionCard({ title, body }: Section) {
  return (
    <div style={{ background: "var(--bg1)", border: "1px solid var(--border)", borderRadius: 14, padding: 20 }}>
      <div style={{ font: "700 14px system-ui", color: "var(--text)", marginBottom: 8 }}>{title}</div>
      <div style={{ font: "500 13px system-ui", color: "var(--text-dim)", lineHeight: 1.6, whiteSpace: "pre-line" }}>{body}</div>
    </div>
  );
}

export default function Guide({ lang }: Props) {
  const uk = lang === "uk";

  const intro = uk
    ? "RisenLab — фан-пайплайн для AI-ремастеру Risen 1 (Piranha Bytes). Точиться на реальні файли гри: витягує текстури/моделі/анімації, дає покращити їх (ШІ або локально), показує результат у 3D до встановлення, і пакує тільки ЗМІНЕНЕ у мінімальний патч — оригінальні файли гри ніколи не переписуються напряму."
    : "RisenLab is a fan pipeline for AI-remastering Risen 1 (Piranha Bytes). It works on the real game files: extracts textures/models/animations, lets you enhance them (AI or local), previews the result in 3D before anything is installed, and packs only what CHANGED into a minimal patch — the game's original files are never overwritten directly.";

  const sections: Section[] = uk
    ? [
        {
          title: "📚 Бібліотека",
          body: "Усі реальні текстури гри одним списком, з деревом папок і пошуком. «AI-регенерація» на одній текстурі або «✨ Покращити всі» на цілій папці/фільтрі — кожна проходить звичайне рев'ю перед тим, як потрапити в патч. Без ШІ-ключа покращення все одно працює (локальне збільшення).",
        },
        {
          title: "🗿 Моделі",
          body: "Реальні меші гри (1666 штук) з деревом папок. Обери модель — побачиш її в 3D з реальними текстурами (текстуровано / каркас / глина / рельєф). Кнопка «✨ Покращити текстури» генерує нові варіанти для всіх матеріалів моделі одразу.",
        },
        {
          title: "🏃 Анімації",
          body: "Обери персонажа й кліп — реальне відтворення скелета. Праворуч: якість анімації (💪 Виразність, 🌊 Вторинний рух, ⚡ Різкість ударів, 🎬 Дрижання, 🎬 60fps), A/B та «поруч» порівняння оригінал/стилізовано, і кнопка зібрати патч. Червона кнопка 60fps-патчу — експериментальна, ще не перевірена в грі.",
        },
        {
          title: "🗂 Рев'ю",
          body: "Кожна AI-регенерація потрапляє в чергу рев'ю, а не одразу в гру. Кнопка «N на рев'ю» вгорі з'являється сама, коли є що переглянути — тисни, коли сам захочеш, воно ніколи не вихопить екран саме. Прийняти / Відхилити (з підтвердженням) / Перегенерувати / Пропустити.",
        },
        {
          title: "⚙️ Налаштування",
          body: "Шлях до Risen.exe (або ярлика), провайдер і ключ ШІ (Replicate або Stability AI — без ключа все одно працює локальне покращення), два режими (Покращити — чесне збільшення без вигадок; ✨ Нові текстури — ШІ малює текстуру заново, з нуля, лишаючи тільки силует), і кнопка «🚀 Текстури в гру» — один клік від прийнятих текстур до реальної гри.",
        },
        {
          title: "Типовий робочий процес",
          body: "1. Налаштування → вкажи шлях до гри, перевір\n2. (Опційно) встав ключ ШІ\n3. Бібліотека/Моделі/Анімації → покращуй, що хочеш\n4. 🗂 Рев'ю → прийми або відхили результати\n5. Налаштування → 🚀 Текстури в гру\n6. Запусти Risen і подивись",
        },
      ]
    : [
        {
          title: "📚 Library",
          body: "Every real texture in the game, one list, with a folder tree and search. \"AI Regenerate\" on a single texture, or \"✨ Enhance all\" on a whole folder/filter — each goes through the normal review before it can reach a patch. Enhancement still works without an AI key (local upscale).",
        },
        {
          title: "🗿 Models",
          body: "The game's real meshes (1666 of them), with a folder tree. Pick one, see it in real 3D with its real textures (textured / wireframe / clay / normal map). \"✨ Enhance textures\" generates new variants for every material on that model at once.",
        },
        {
          title: "🏃 Animations",
          body: "Pick a character and a clip — real skeleton playback. On the right: animation quality (💪 Expressiveness, 🌊 Secondary motion, ⚡ Strike sharpness, 🎬 Jitter, 🎬 60fps), A/B and side-by-side original/styled compare, and a button to build a patch. The red 60fps-patch button is experimental — not yet verified in-game.",
        },
        {
          title: "🗂 Review",
          body: "Every AI regeneration lands in a review queue, not straight into the game. The \"N to review\" button up top appears on its own whenever there's something to look at — click it whenever YOU want, it never hijacks the screen on its own. Approve / Reject (with confirmation) / Regenerate / Skip.",
        },
        {
          title: "⚙️ Settings",
          body: "Path to Risen.exe (or a shortcut), AI provider and key (Replicate or Stability AI — local enhancement still works without one), two modes (Enhance — an honest upscale, invents nothing; ✨ New textures — the AI repaints the texture from scratch, keeping only the silhouette), and a \"🚀 Ship to game\" button — one click from approved textures to the real game.",
        },
        {
          title: "Typical workflow",
          body: "1. Settings → point at the game, check it\n2. (Optional) paste an AI key\n3. Library/Models/Animations → enhance whatever you want\n4. 🗂 Review → approve or reject the results\n5. Settings → 🚀 Ship to game\n6. Launch Risen and look",
        },
      ];

  return (
    <div style={{ flex: 1, overflow: "auto", padding: "30px 0", display: "flex", justifyContent: "center" }}>
      <div style={{ width: 680, display: "flex", flexDirection: "column", gap: 16 }}>
        <div>
          <div style={{ font: "700 22px system-ui", color: "var(--text)" }}>RisenLab</div>
          <div style={{ font: "500 13.5px system-ui", color: "var(--text-dim)", lineHeight: 1.6, marginTop: 8 }}>{intro}</div>
        </div>

        {sections.map((s) => (
          <SectionCard key={s.title} {...s} />
        ))}

        <div style={{ textAlign: "center", padding: "10px 0 4px", font: "600 12px system-ui", color: "var(--text-faint)" }}>
          {uk ? "Автор: " : "Author: "}
          <span style={{ color: "var(--accent)" }}>Sector</span>
        </div>
      </div>
    </div>
  );
}
