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
