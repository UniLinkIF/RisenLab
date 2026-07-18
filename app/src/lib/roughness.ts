// Converts a legacy specular-intensity map (the real game's `..._Specular_NN` textures — see
// `deriveSpecularName` in materials.ts) into a PBR roughness map for three.js's
// `MeshStandardMaterial.roughnessMap`. Previously every material in both 3D viewers used one
// flat hardcoded `roughness: 0.75` regardless of what it actually was (a metal blade and a
// cloth cloak read identically) — this at least varies per real material.
//
// CAVEAT (be upfront about this, don't oversell it): a specular map's exact channel semantics
// in Genome (plain intensity vs. tinted specular color vs. a packed gloss/shininess value)
// aren't confirmed from the file format alone the way the DXT5nm normal-map swizzle was
// (that one had a hard empirical signature — R/B always exactly 0 — this doesn't). This is a
// reasonable default reading (brighter specular = shinier = lower roughness), not a proven one.

/** Mutates `data` (RGBA8, interleaved) in place: desaturates to luminance, then inverts it into
 * a roughness value (bright specular/shiny -> low roughness; dark specular/matte -> high
 * roughness) — the opposite sense from the source texture's own brightness. Clamped to
 * `[MIN_ROUGHNESS, MAX_ROUGHNESS]`: the literal extremes (a perfect mirror or a fully flat
 * matte surface) read as visibly wrong on real game props far more often than a specular map
 * author actually intended a true 0 or 1. */
const MIN_ROUGHNESS = 0.12;
const MAX_ROUGHNESS = 0.92;

export function specularLuminanceToRoughness(data: Uint8ClampedArray): void {
  for (let i = 0; i < data.length; i += 4) {
    const luminance = (0.2126 * data[i] + 0.7152 * data[i + 1] + 0.0722 * data[i + 2]) / 255;
    const roughness = MAX_ROUGHNESS - luminance * (MAX_ROUGHNESS - MIN_ROUGHNESS);
    const byte = Math.round(roughness * 255);
    data[i] = byte;
    data[i + 1] = byte;
    data[i + 2] = byte;
    data[i + 3] = 255;
  }
}
