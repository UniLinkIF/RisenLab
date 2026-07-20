// Which showroom zone a real game mesh/actor belongs in — grounded in the REAL folder names
// confirmed against the owner's actual connected game (`list-meshes`/`list-actors` against the
// real Steam install, 2026-07-20): Items_Weapons_Swords_01 (49), Items_Weapons_Shields_01 (8),
// Items_Weapons_Axes_01 (12), Items_Weapons_Staffs_01 (10), Items_Weapons_Ammo_01 (2),
// Items_Helmets_01 (7), Items_01 (120 — a real mixed bag: food, valuables, tools, books,
// potions), Items_Plants_01 (18), and actors under _emfx36/{Monster,Humans}/Bodys.
// Everything else (Levelmesh_*, Objects_Misc_01/Nat_01/Interacts_01, Editor_*, Testkram_*,
// _emfx36/Heads, _emfx36/Items) is level geometry/editor/technical cruft, not a real
// standalone "item" worth putting on display — deliberately excluded. `_emfx36/Mobsis/Bodys`
// is ALSO excluded: despite the name and despite being real .xmac actors, every entry there
// (verified against the owner's connected game, 2026-07-20 — all 20 of them) is an
// `Object_Interact_Animated_*` rig — a winch, a well crank, a sarcophagus lid, a treasure
// chest — not a creature, so it doesn't belong in a "characters" room. It was the real,
// fully-diagnosed cause of a chunk of the owner-reported "white models" in the figures room:
// a rigged prop's skeleton bind pose can look vaguely humanoid in silhouette, but it has no
// body/cloth diffuse material to resolve, so it renders as the plain white fallback forever
// (confirmed live: not a texture-load timing issue — the pop-in had long finished).
import type { ActorEntry, MeshEntry } from "./types";

export type ItemZoneId = "swords" | "shields" | "weaponsMisc" | "food" | "valuables" | "tools";
export type ActorZoneId = "humans" | "monsters" | "mobs";

const FOOD_KEYWORDS = [
  "bread", "cheese", "meat", "turkey", "sausage", "wine", "milk", "grape", "apple", "carrot",
  "onion", "potato", "beet", "egg", "herring", "plaice", "stew", "chickenleg", "rum", "waterbag",
];
const VALUABLE_KEYWORDS = [
  "gold", "ruby", "diamond", "pearl", "necklace", "ring_", "ring.", "smaragd", "amber", "crystal",
  "goldpile", "goldcoin",
];

/** Categorizes one `Items_01`/`Items_Plants_01` prop by its own real name — these two folders are
 * a genuine mixed bag (food next to jewelry next to a goldsmith's hammer), so folder alone isn't
 * enough to make a coherent table/shelf. */
function categorizeProp(name: string): ItemZoneId {
  const lower = name.toLowerCase();
  if (FOOD_KEYWORDS.some((k) => lower.includes(k))) return "food";
  if (VALUABLE_KEYWORDS.some((k) => lower.includes(k))) return "valuables";
  return "tools";
}

export function categorizeMesh(entry: MeshEntry): ItemZoneId | null {
  switch (entry.folder) {
    case "Items_Weapons_Swords_01":
      return "swords";
    case "Items_Weapons_Shields_01":
      return "shields";
    case "Items_Weapons_Axes_01":
    case "Items_Weapons_Staffs_01":
    case "Items_Weapons_Ammo_01":
    case "Items_Helmets_01":
      return "weaponsMisc";
    case "Items_01":
    case "Items_Plants_01":
      return categorizeProp(entry.name);
    default:
      return null;
  }
}

export function categorizeActor(entry: ActorEntry): ActorZoneId | null {
  // Real folders look like "_emfx36/Monster/Bodys" — substring match, not equality, since the
  // exact depth/casing isn't guaranteed stable across every archive. `Mobsis` is deliberately
  // NOT mapped to "mobs" — see the module doc comment for why (it's interactive-object rigs,
  // not creatures).
  if (entry.folder.includes("Humans")) return "humans";
  if (entry.folder.includes("Monster")) return "monsters";
  return null;
}
