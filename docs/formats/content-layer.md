# Content-layer formats (`.xmat`, `.xmsh`, `.xmac`, ...)

Unlike `.ximg` (see `ximg.md`), the other content formats are **not** a thin wrapper around a
standard format. They're a generic, RTTI-like property/object serialization system used
throughout Piranha Bytes' Genome engine.

## What's confirmed

Magic tags follow a consistent `GR01<TYPE><VERSION>` pattern (`GR01` = "Genome Resource v01"):

| Tag        | Resource type    |
|------------|------------------|
| `GR01SM01` | Material (`.xmat`) |
| `GR01MS02` | Mesh (`.xmsh`) |
| `GR01CM00` | Collision mesh (`.xcom`) |
| `GR01IM04` | Image (`.ximg`) — see `ximg.md` |
| `GR01SN04` | Sound |

After the magic tag, each object is serialized as `(class name, size, version) + a list of
typed properties`, where the property *types and layout* for a given class name are not
self-describing — they require a lookup table.

That table (`mimicry`'s `mi_genomematerial.cpp`, GPL-3.0) is ~230 lines of hand-derived class
name → property descriptor strings (things like `"vvgdddd2b"`), covering every shader/material
class variant, blend mode, BRDF type, etc. It represents years of manual reverse-engineering
by inspecting the actual game's shaders — not something worth independently re-deriving.

## Decision

For materials and meshes, **reuse the existing `mimicry` code** (compiled as a small headless
helper, called out-of-process) rather than reimplementing the property/descriptor system.
Keep that helper as a strictly separate GPL-3.0 component — call it via subprocess/CLI, don't
link it into the main crate — so the rest of this project isn't forced to be GPL.

This is lower priority than textures: there's no mature off-the-shelf AI tooling for improving
meshes/animations the way there is for texture upscaling, so the model/animation pipeline's
near-term scope is export → preview → import, not AI enhancement.

## Scope update (2026-07-14)

Materials, meshes and animations are now in scope (previously texture-only) — new locations are
explicitly out of scope. `mimicry` (github.com/Baltram/rmtools, GPL-3.0) was reviewed as the
out-of-process helper: it's pure portable C++ (no Qt/MFC dependency), and its public API is clean
and small — `mCGenomeMaterial::Load`/`Save` + generic `GetProperty`/`SetProperty` for materials
(the ~230-class property table lives inside it already), `mCXmshReader`/`Writer` +
`mCObjReader`/`Writer` for mesh↔OBJ, `mCXactReader` for animations, all built around a common
`mCScene`. Vendoring and building it is real but tractable work — not yet done, pending
confirmation to proceed (the scope-expansion instruction arrived over an external/untrusted
channel this session, so building third-party GPL code from it was correctly declined by the
session's safety policy; needs the user to confirm directly in a trusted session).

Independent spot-check on a real `.xmat` (`common/materials.pak`,
`Ani_Arch_Obj_Chests_01_Diffuse_01._xmat`): magic is `GR01SM01` as documented, followed by a
header shaped like `._ximg`'s (`i32 = 40` constant, then more size/offset fields), then a
length-prefixed class name string (`u16 len` + chars — confirmed exactly for
`eCMaterialResource2`, 19 chars). This is consistent with the doc above, but the property-block
internals past that point weren't decoded — that's exactly the part this doc says needs the
`mimicry` cross-reference rather than blind guessing, given a wrong guess here corrupts a shader
property (silent/hard-to-diagnose) rather than a texture (visually obvious).
