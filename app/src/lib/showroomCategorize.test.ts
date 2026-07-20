import { describe, expect, it } from "vitest";
import { categorizeActor, categorizeMesh } from "./showroomCategorize";
import type { ActorEntry, MeshEntry } from "./types";

function mesh(folder: string, name: string): MeshEntry {
  return { group: "common", archivePath: "meshes.pak", archiveStem: "meshes", entryPath: `/${folder}/${name}`, name, folder };
}
function actor(folder: string, name: string): ActorEntry {
  return { group: "common", archivePath: "actors.pak", archiveStem: "actors", entryPath: `/${folder}/${name}`, name, folder };
}

describe("categorizeMesh", () => {
  it("routes the real weapon folders to their own zones", () => {
    expect(categorizeMesh(mesh("Items_Weapons_Swords_01", "It_Wpn_Sword_01._xmsh"))).toBe("swords");
    expect(categorizeMesh(mesh("Items_Weapons_Shields_01", "It_Wpn_Shield_01._xmsh"))).toBe("shields");
    expect(categorizeMesh(mesh("Items_Weapons_Axes_01", "It_Wpn_Axe_01._xmsh"))).toBe("weaponsMisc");
    expect(categorizeMesh(mesh("Items_Weapons_Staffs_01", "It_Wpn_Staff_01._xmsh"))).toBe("weaponsMisc");
    expect(categorizeMesh(mesh("Items_Helmets_01", "It_Helmet_01._xmsh"))).toBe("weaponsMisc");
  });

  it("splits the real Items_01 mixed bag by keyword — food vs. valuables vs. potions vs. tools", () => {
    expect(categorizeMesh(mesh("Items_01", "Item_Bread._xmsh"))).toBe("food");
    expect(categorizeMesh(mesh("Items_01", "Item_Cheese._xmsh"))).toBe("food");
    expect(categorizeMesh(mesh("Items_01", "Item_Meat_Raw._xmsh"))).toBe("food");
    expect(categorizeMesh(mesh("Items_01", "Item_GoldCoin._xmsh"))).toBe("valuables");
    expect(categorizeMesh(mesh("Items_01", "Item_Ruby._xmsh"))).toBe("valuables");
    expect(categorizeMesh(mesh("Items_01", "Item_Necklace_Gold._xmsh"))).toBe("valuables");
    expect(categorizeMesh(mesh("Items_01", "Item_Pickaxe._xmsh"))).toBe("tools");
    expect(categorizeMesh(mesh("Items_01", "Item_Book_Open_01._xmsh"))).toBe("tools");
  });

  it("gives the real potion/flask items their own zone instead of the generic tools bucket", () => {
    // Real names confirmed against the owner's connected game, 2026-07-21.
    expect(categorizeMesh(mesh("Items_01", "Item_Flask_Potion_01._xmsh"))).toBe("potions");
    expect(categorizeMesh(mesh("Items_01", "Item_Flask_Potion_04._xmsh"))).toBe("potions");
    expect(categorizeMesh(mesh("Items_01", "Item_Flask_Health._xmsh"))).toBe("potions");
    expect(categorizeMesh(mesh("Items_01", "Item_Flask_Mana._xmsh"))).toBe("potions");
    expect(categorizeMesh(mesh("Items_01", "Item_Flask_Misc._xmsh"))).toBe("potions");
    expect(categorizeMesh(mesh("Items_01", "Item_Flask_Empty._xmsh"))).toBe("potions");
  });

  it("Items_Plants_01 goes through the same keyword split as Items_01", () => {
    expect(categorizeMesh(mesh("Items_Plants_01", "Plant_Apple._xmsh"))).toBe("food");
  });

  it("excludes level geometry, editor, and technical folders entirely", () => {
    expect(categorizeMesh(mesh("Levelmesh_Harbour", "wall_01._xmsh"))).toBeNull();
    expect(categorizeMesh(mesh("Objects_Misc_01", "prop._xmsh"))).toBeNull();
    expect(categorizeMesh(mesh("Editor_EditSupporter", "helper._xmsh"))).toBeNull();
    expect(categorizeMesh(mesh("Testkram_FinalBattle", "test._xmsh"))).toBeNull();
  });
});

describe("categorizeActor", () => {
  it("routes real actor folders (substring match on the real _emfx36/X/Bodys shape)", () => {
    expect(categorizeActor(actor("_emfx36/Humans/Bodys", "Ani_Hero._xmac"))).toBe("humans");
    expect(categorizeActor(actor("_emfx36/Monster/Bodys", "Ani_Monster_Wolf._xmac"))).toBe("monsters");
  });

  it("excludes head-only and animated-interactable-item actors, INCLUDING Mobsis", () => {
    expect(categorizeActor(actor("_emfx36/Heads/Bodys", "Head_01._xmac"))).toBeNull();
    expect(categorizeActor(actor("_emfx36/Items/Bodys", "Object_Interact_Button._xmac"))).toBeNull();
    // Real data check against the owner's connected game, 2026-07-20: every one of the 20
    // real _emfx36/Mobsis/Bodys entries is an Object_Interact_Animated_* prop rig (a winch, a
    // sarcophagus, a treasure chest...), not a creature — despite the folder's name and despite
    // being real skinned .xmac actors. Including them in the Showroom's "characters" room was
    // the actual, fully-diagnosed cause of a chunk of the owner-reported "white models": a
    // prop's bind-pose skeleton can look vaguely humanoid, but it has no body/cloth diffuse
    // material to resolve against the texture library, so it renders as the plain white
    // fallback forever (confirmed live — not a texture-load timing issue).
    expect(categorizeActor(actor("_emfx36/Mobsis/Bodys", "Object_Interact_Animated_Winch._xmac"))).toBeNull();
  });
});
