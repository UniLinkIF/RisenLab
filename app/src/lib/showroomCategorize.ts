// Which showroom zone a real game mesh/actor belongs in — grounded in the REAL folder names
// confirmed against the owner's actual connected game (`list-meshes`/`list-actors` against the
// real Steam install, 2026-07-20): Items_Weapons_Swords_01 (49), Items_Weapons_Shields_01 (8),
// Items_Weapons_Axes_01 (12), Items_Weapons_Staffs_01 (10), Items_Weapons_Ammo_01 (2),
// Items_Helmets_01 (7), Items_01 (120 — a real mixed bag: food, valuables, tools, books,
// potions), Items_Plants_01 (18), and actors under _emfx36/{Monster,Humans}/Bodys.
// Everything else (Levelmesh_*, Objects_Misc_01/Nat_01/Interacts_01, Editor_*, Testkram_*,
// _emfx36/Heads, _emfx36/Items) is level geometry/editor/technical cruft, not a real
// standalone "item" worth putting on display — deliberately excluded. `_emfx36/Mobsis/Bodys`
// is a real, separate `"props"` zone (2026-07-21) rather than either "excluded" or "mobs":
// despite the name and despite being real .xmac actors, every entry there (verified against
// the owner's connected game, 2026-07-20 — all 20 of them) is an `Object_Interact_Animated_*`
// rig — a winch, a well crank, a sarcophagus lid, a treasure chest — not a creature, so it
// doesn't belong in the "characters" room (that WAS the real, fully-diagnosed cause of a chunk
// of the owner-reported "white models" there: a rigged prop's skeleton bind pose can look
// vaguely humanoid in silhouette, but has no body/cloth diffuse material to resolve against —
// confirmed live, not a texture-load timing issue). The owner explicitly asked (2026-07-20) for
// the Showroom to eventually show these too, just not mislabeled as people — a dedicated
// fourth room, not a re-inclusion into "characters".
import type { ActorEntry, MeshEntry } from "./types";

export type ItemZoneId = "swords" | "shields" | "weaponsMisc" | "food" | "valuables" | "potions" | "tools";
export type ActorZoneId = "humans" | "monsters" | "mobs" | "props";

const FOOD_KEYWORDS = [
  "bread", "cheese", "meat", "turkey", "sausage", "wine", "milk", "grape", "apple", "carrot",
  "onion", "potato", "beet", "egg", "herring", "plaice", "stew", "chickenleg", "rum", "waterbag",
];
const VALUABLE_KEYWORDS = [
  "gold", "ruby", "diamond", "pearl", "necklace", "ring_", "ring.", "smaragd", "amber", "crystal",
  "goldpile", "goldcoin",
];
// Real names confirmed against the owner's connected game (2026-07-21): Item_Flask_Potion_01-04,
// Item_Flask_Health, Item_Flask_Mana, Item_Flask_Misc, Item_Flask_Empty — all under Items_01,
// all sharing the "Flask" name segment. Previously fell into the generic "tools" bucket; owner
// asked (2026-07-20 session) to see them on their own shelf.
const POTION_KEYWORDS = ["flask"];

/** Categorizes one `Items_01`/`Items_Plants_01` prop by its own real name — these two folders are
 * a genuine mixed bag (food next to jewelry next to a goldsmith's hammer), so folder alone isn't
 * enough to make a coherent table/shelf. */
function categorizeProp(name: string): ItemZoneId {
  const lower = name.toLowerCase();
  if (POTION_KEYWORDS.some((k) => lower.includes(k))) return "potions";
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

// `Ani_Hero_Skeleton` is the ONE entry in the real `_emfx36/Humans/Bodys` folder that doesn't
// follow the `Ani_Hero_Armor_*` naming every other real human variant uses (confirmed 2026-07-21
// by converting it and reading its own real .mtl: a single `EMFX_Default` material with no
// `map_Kd` line at all — a bare animation-rigging reference, not a dressed, texturable
// character). It's a real, CONFIRMED (not guessed) contributor to the "white models" in the
// figures room — every other white figure checked the same way (Woman_Peasant, Woman_Slave,
// Don_Hunter, Guard) had real, correctly-resolvable `map_Kd` references, so this specific file
// is excluded rather than assuming the whole texture-resolution path is broken.
const BARE_RIG_ACTOR_NAMES = new Set(["ani_hero_skeleton"]);

export function categorizeActor(entry: ActorEntry): ActorZoneId | null {
  if (BARE_RIG_ACTOR_NAMES.has(entry.name.replace(/\._xmac$/i, "").toLowerCase())) return null;
  // Real folders look like "_emfx36/Monster/Bodys" — substring match, not equality, since the
  // exact depth/casing isn't guaranteed stable across every archive. `Mobsis` is deliberately
  // NOT mapped to "mobs" (see the module doc comment for why — it's interactive-object rigs,
  // not creatures) but IS its own "props" zone (2026-07-21, owner request: show everything,
  // just not mislabeled as a character) — a real fourth Showroom room, not the figures room.
  if (entry.folder.includes("Humans")) return "humans";
  if (entry.folder.includes("Monster")) return "monsters";
  if (entry.folder.includes("Mobsis")) return "props";
  return null;
}
