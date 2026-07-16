// Genome (Risen's D3D-era engine) stores normal maps DXT5-compressed with a swizzle that
// trades the color block's R/B channels (set to 0, unused) for the alpha block's much higher
// per-pixel precision: X lives in alpha, Y lives in green, Z (the "into the surface" component)
// is dropped entirely and must be reconstructed at render time — this is the well-known
// "DXT5nm" packing. Confirmed empirically across every normal map sampled from this game's own
// archives (axes, generic item props, architecture): R and B are always exactly 0, while G and
// A both carry real, varying relief data (mean ~127, the signature of a normalized [-1,1]
// component stored as a byte). Three.js's `MeshStandardMaterial.normalMap` has no idea about
// this swizzle — it reads R/G/B directly as (X,Y,Z), so without unpacking this first, every
// pixel decodes to X=-1 (from R=0) and Z=-1 (from B=0): a normal tilted to an extreme grazing
// angle on every single texel, which is exactly why models with a normal map applied rendered
// as a near-black silhouette (self-shadowed under directional light) even though the diffuse
// texture underneath was completely fine.
export function looksDxt5nmSwizzled(data: Uint8ClampedArray): boolean {
  // Sampling every 37th texel (not every pixel) keeps this cheap even on a 1024x1024 atlas,
  // while 37 being coprime with typical power-of-two row lengths avoids only ever landing on
  // the same column.
  const stride = 37 * 4;
  for (let i = 0; i < data.length; i += stride) {
    if (data[i] !== 0 || data[i + 2] !== 0) return false;
  }
  return true;
}

/** Mutates `data` (RGBA8, interleaved) in place from Genome's swizzled DXT5nm layout
 * (R unused, G=Y, B unused, A=X) into a standard tangent-space normal map (R=X, G=Y, B=Z),
 * reconstructing Z from the unit-length constraint since it was never stored. */
export function reconstructTangentNormalMap(data: Uint8ClampedArray): void {
  for (let i = 0; i < data.length; i += 4) {
    const x = (data[i + 3] / 255) * 2 - 1;
    const y = (data[i + 1] / 255) * 2 - 1;
    const z = Math.sqrt(Math.max(0, 1 - x * x - y * y));
    data[i] = Math.round((x * 0.5 + 0.5) * 255);
    data[i + 2] = Math.round((z * 0.5 + 0.5) * 255);
    data[i + 3] = 255;
  }
}
