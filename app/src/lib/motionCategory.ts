// Real clip-name grammar (from the actual 2333 body clips in animations.pak):
// `<Creature>_<Stance>_<?>_<Weapon>_<S#/P#>_<ACTION>_<phase>_<direction>...` — the ACTION
// token(s) carry the meaning (Move_Walk, StormAttack, Stumble, Ambient_Loop, ...). Position
// isn't reliable (S0/S1 weapon-stance markers shift it), so classification is keyword-based
// over the whole name, with the match order resolving mixed names (e.g.
// "SlideAttack1_Hit" is an attack, not a hit reaction).

export type MotionCategory = "all" | "move" | "combat" | "react" | "idle" | "other";

export const MOTION_CATEGORIES: { id: MotionCategory; uk: string; en: string }[] = [
  { id: "all", uk: "Всі", en: "All" },
  { id: "move", uk: "🚶 Рух", en: "🚶 Move" },
  { id: "combat", uk: "⚔️ Бій", en: "⚔️ Combat" },
  { id: "react", uk: "💥 Реакції", en: "💥 Reactions" },
  { id: "idle", uk: "🧍 Побут", en: "🧍 Idle" },
  { id: "other", uk: "Інше", en: "Other" },
];

const COMBAT = /(attack|parade|warn|roar|reload|holdright|holdleft|hold_begin|hold_end|stomp|grenade|flamethrower|finishhim|_kill)/;
const REACT = /(stumble|_hit_|_dead|death|fall)/;
const MOVE = /(move_|_jump|sneak|turn)/;
const IDLE = /(ambient|_eat_|eat_loop|eat_begin|eat_end|sleep|listen|_say_|enjoy|sitground|sitbench|sitthrone|stand_begin|stand_end|lizardbook)/;

export function motionCategory(name: string): Exclude<MotionCategory, "all"> {
  const n = name.toLowerCase();
  if (COMBAT.test(n)) return "combat";
  if (REACT.test(n)) return "react";
  if (MOVE.test(n)) return "move";
  if (IDLE.test(n)) return "idle";
  return "other";
}
