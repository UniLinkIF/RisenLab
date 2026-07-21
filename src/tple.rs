//! Risen 1 `._tple` template file — read-only scan for **script-hook bindings** (the
//! `PostInteractScript`/`InteractScript`/... family of properties that decide what happens
//! when the player or an NPC interacts with an item or world object).
//!
//! This is deliberately narrow, not a general property-tree reader: the full GENOMFLE/GENOMETP
//! reflection format (see `Engine.dll`'s `eCEntity::OnRead` / `SharedBase.dll`'s
//! `bCAccessorPropertyObject::Read`, reverse-engineered via Ghidra) mixes a packed binary value
//! blob with a separate reflection/schema string table, and fully parsing the binary value blob
//! (to answer "what enum number is UseType") is still unsolved. But every *script-proxy* typed
//! property (the ones this scan cares about) has an unusual, useful property: its bound value
//! (a compiled function name, e.g. `InventoryUse_Player_ConsumeBread`) is ALSO stored as a plain
//! string, immediately following the property's own name string, in that same schema/string
//! region — real data confirmed this on `It_Bread.tple` (`PostInteractScript` immediately
//! followed by `InventoryUse_Player_ConsumeBread`) vs. `It_Flute.tple` (`PostInteractScript`
//! immediately followed by `PreInventoryUse_Player` — a string that LOOKS like a function name
//! but isn't one; only `PreInventoryUse_Player_Transform` actually exists as a compiled export,
//! confirmed against real `Script_Game.dll` symbols. This is the actual, real bug that makes the
//! flute item do nothing when used — not a RisenLab bug, a leftover from the original game).
//!
//! Detecting "is the next string a real bound value, or just the next schema field's own name"
//! uses the same two rules a human scanning the raw bytes would use: a schema/type descriptor
//! string always contains `<` (e.g. `bTPropertyContainer<enum gEInteractionType>`), and every
//! other schema *field name* this scan can encounter is in the fixed list below (compiled from
//! every real `.tple` this project has inspected this session — `It_Flute`, `It_Bread`,
//! `It_Po_Health_01`, `It_Saringda`, `FP_Flute`, `FP_Sit_Ground`, `FP_Guard`). A value string
//! never coincides with a real property name in any file seen so far.
//!
//! Strings themselves are `u16` little-endian length prefix + raw ASCII bytes, no terminator —
//! the same convention already established for `.xmot`/`.xmac` node names in this project,
//! confirmed empirically against real `.tple` bytes (not from any spec).

use serde::Serialize;
use std::collections::HashSet;

/// Every schema field name this scan can walk past that is NOT itself a script-hook property —
/// i.e. things that can legitimately follow a script-hook's *name* without being its *value*.
/// Deliberately over-inclusive (a stray real value that happens to match one of these would be
/// mis-read as "unbound") rather than under-inclusive (which would mis-read a schema field name
/// as if it were somebody's bound function, a much more misleading failure).
fn known_schema_field(s: &str) -> bool {
    const NAMES: &[&str] = &[
        "Amount", "GoldValue", "MissionItem", "Permanent", "SortValue", "Category", "IconImage",
        "HoldOffset", "Dropped", "ItemWorld", "ItemInventory", "Spell", "RequiredSkills",
        "ModifySkills", "Modifier", "Skill", "UseType", "CanEquipScript", "EquipScript",
        "UnEquipScript", "EffectMaterial", "HoldType", "IsDangerousWeapon", "CombatHitRangeOffset",
        "EnterROIScript", "ExitROIScript", "TouchScript", "IntersectScript", "UntouchScript",
        "TriggerScript", "UntriggerScript", "DamageScript", "CanAttachSlotScript",
        "AttachedSlotScript", "DetachedSlotScript", "RoomChangedScript", "RoutineTask",
        "GroundBias", "FocusPriority", "FocusNameType", "FocusNameBone", "FocusViewOffset",
        "FocusWorldOffset", "FocusPriorityScript", "Slots", "InteractionCounter", "Owner",
        "Type", "CanInteractScript", "PreInteractScript", "InteractScript", "PostInteractScript",
        "CanInteract_Magic_Item", "PreInteract_Magic_Item", "CanQuickUse_Player",
        "QuickUse_Player", "NavTestResult", "StaticIlluminated", "ShadowCasterType",
        "CastDirLightShadows", "CastPntLightShadows", "CastStaticShadows",
        "CastDirLightShadowsOverwrite", "CastPntLightShadowsOverwrite",
        "CastStaticShadowsOverwrite", "MeshFileName", "MaterialSwitch", "SubMeshCulling",
        "Lightmaped", "EnableRadiosity", "MaxSubMeshTriangles", "UnitsPerLightmapTexel",
        "LevelOfDetailRange0", "LevelOfDetailRange1", "LevelOfDetailRange2", "EnableDecals",
        "HitByProjectile", "TotalMass", "MassSpaceInertia", "StartVelocity",
        "StartAngularVelocity", "StartForce", "StartTorque", "WakeUpCounter", "LinearDamping",
        "AngularDamping", "MaxAngularVelocity", "CenterOfMass", "CCDMotionTreshold", "BodyFlag",
        "PhysicsEnabled", "Group", "Range", "DisableCollision", "DisableResponse",
        "IgnoredByTraceRay", "IsUnique", "IsClimbable", "ShapeType", "Material",
        "ShapeAABBAdaptMode", "EnableCCD", "OverrideEntityAABB", "TriggersOnTouch",
        "TriggersOnUntouch", "TriggersOnIntersect", "SkinWidth", "IsLazyGenerated", "FileVersion",
        // Bare type-name strings (as opposed to the `bTPropertyContainer<...>`-style ones,
        // already caught by the `<` check below): a property whose script-proxy-typed slot is
        // unbound is followed by ITS OWN type name, not the next property's name — e.g. a real,
        // unbound `PreInteractScript` is followed by `gCScriptProxyAIFunction`. Without this, an
        // unbound property gets misreported as if its own type name were a real bound value.
        "eCScriptProxyScript", "gCScriptProxyAIFunction", "gCScriptProxyAIState", "eCEntityProxy",
        "eCTemplateEntityProxy", "eCGuiBitmapProxy2", "bCString", "bCVector", "bool", "int",
        "long", "short", "float",
    ];
    NAMES.contains(&s)
}

/// The specific properties worth surfacing — the "does using this thing actually do anything"
/// hooks. `FocusPriorityScript`/`CanAttachSlotScript`/etc. exist in every item and are almost
/// always unbound by design (not every item can be a slot target), so scanning ALL script-typed
/// fields would bury the interesting ones in noise; this is the subset an owner actually cares
/// about when asking "why doesn't this item do anything when I use it".
const INTERESTING_HOOKS: &[&str] =
    &["CanInteractScript", "PreInteractScript", "InteractScript", "PostInteractScript"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptBinding {
    pub property: String,
    /// `None` means the property's own name was immediately followed by another schema field's
    /// name (or a `<...>` type descriptor) — i.e. nothing was ever written here, the hook is
    /// unbound. `Some(name)` that isn't a real compiled function is exactly the flute's bug.
    pub bound_value: Option<String>,
}

/// A single `[u16 len][ascii bytes]` token, found by scanning (not by following any offset
/// table — this format has none of those for the string region, confirmed by every `.tple`
/// this project has walked byte-by-byte this session).
fn read_tokens(data: &[u8]) -> Vec<(usize, String)> {
    let mut tokens = Vec::new();
    let mut i = 0usize;
    while i + 2 <= data.len() {
        let len = u16::from_le_bytes([data[i], data[i + 1]]) as usize;
        if len > 0 && i + 2 + len <= data.len() {
            let chunk = &data[i + 2..i + 2 + len];
            if chunk.iter().all(|&b| (0x20..0x7f).contains(&b)) {
                let s = String::from_utf8_lossy(chunk).into_owned();
                tokens.push((i, s));
                i += 2 + len;
                continue;
            }
        }
        i += 1;
    }
    tokens
}

/// Scans a `.tple` file for the four interaction script hooks (`CanInteractScript`,
/// `PreInteractScript`, `InteractScript`, `PostInteractScript`) and reports each one found, with
/// its bound value if any. Every occurrence is reported (a `Slots` array can hold more than one
/// interaction, e.g. NPCs vs. player, each with its own set of these four).
pub fn scan_script_bindings(data: &[u8]) -> Vec<ScriptBinding> {
    let tokens = read_tokens(data);
    let mut out = Vec::new();
    let hooks: HashSet<&str> = INTERESTING_HOOKS.iter().copied().collect();
    for (idx, (_, name)) in tokens.iter().enumerate() {
        if !hooks.contains(name.as_str()) {
            continue;
        }
        let bound_value = tokens.get(idx + 1).and_then(|(_, next)| {
            let is_schema = next.contains('<') || known_schema_field(next);
            if is_schema {
                None
            } else {
                Some(next.clone())
            }
        });
        out.push(ScriptBinding { property: name.clone(), bound_value });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_stream(names: &[&str]) -> Vec<u8> {
        let mut data = Vec::new();
        for n in names {
            data.extend_from_slice(&(n.len() as u16).to_le_bytes());
            data.extend_from_slice(n.as_bytes());
        }
        data
    }

    #[test]
    fn bound_hook_reports_its_real_value() {
        let data = token_stream(&[
            "InteractScript",
            "Interact_Player_TakeItem",
            "PostInteractScript",
            "InventoryUse_Player_ConsumeBread",
            "CanInteract_Magic_Item",
        ]);
        let found = scan_script_bindings(&data);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].property, "InteractScript");
        assert_eq!(found[0].bound_value.as_deref(), Some("Interact_Player_TakeItem"));
        assert_eq!(found[1].property, "PostInteractScript");
        assert_eq!(found[1].bound_value.as_deref(), Some("InventoryUse_Player_ConsumeBread"));
    }

    #[test]
    fn unbound_hook_followed_by_schema_field_reports_none() {
        // Real shape seen on It_Saringda.tple's CanEquipScript (unbound): the very next token
        // is a `<...>` type descriptor, not a value.
        let data = token_stream(&[
            "CanInteractScript",
            "bTPropertyContainer<enum gEInteractionType>",
            "PreInteractScript",
        ]);
        let found = scan_script_bindings(&data);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].bound_value, None);
        // The last one has no successor token at all — must not panic, must report unbound.
        assert_eq!(found[1].bound_value, None);
    }

    #[test]
    fn flutes_real_suspicious_binding_is_reported_as_a_value_not_hidden() {
        // Real bytes from It_Flute.tple: PostInteractScript is immediately followed by
        // "PreInventoryUse_Player" — a string that looks like a function but isn't a real
        // compiled one. This scanner's job is only to report the raw binding, not to judge it;
        // that's why this is `Some("PreInventoryUse_Player")`, same as a real one would be.
        let data = token_stream(&[
            "PostInteractScript",
            "PreInventoryUse_Player",
            "InventoryUse_Player",
            "CanInteract_Magic_Item",
        ]);
        let found = scan_script_bindings(&data);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].bound_value.as_deref(), Some("PreInventoryUse_Player"));
    }

    #[test]
    fn unbound_hook_followed_by_its_own_bare_type_name_reports_none() {
        // Real bytes from every .tple this project has inspected: an unbound PreInteractScript
        // is followed by "gCScriptProxyAIFunction" (its own type name, not a `<...>`-style
        // generic container) — this must not be mistaken for a genuine bound function.
        let data = token_stream(&["PreInteractScript", "gCScriptProxyAIFunction", "InteractScript"]);
        let found = scan_script_bindings(&data);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].property, "PreInteractScript");
        assert_eq!(found[0].bound_value, None);
    }

    #[test]
    fn no_hooks_present_returns_empty() {
        let data = token_stream(&["Amount", "GoldValue"]);
        assert!(scan_script_bindings(&data).is_empty());
    }
}
