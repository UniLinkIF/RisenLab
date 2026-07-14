# `._ximg` texture format (Risen 1/2)

Field offsets cross-referenced against `QImageIOPlugin/ximgplugin/ximghandler.cpp`
(`rmtools`, GPL-3.0) purely to confirm the layout; `src/ximg.rs` is an independent
implementation (no GPL code included or linked).

Verified against 5 real files extracted from a licensed Risen 1 install
(`EditorBilboard_EVT_Sound_{Forest,Stones,Waterdropes,Waves,Wind}._ximg`): all parsed,
all extracted DDS payloads opened correctly in Pillow as valid 64x64 RGBA images.

## Layout (little-endian)

```
offset 0  : "GR01IM04"           8 bytes, magic (Genome Resource, ImageResource v04)
offset 8  : i32 = 40             resource header size, constant across all 5 samples
offset 12 : i32                  property block size (225 in all 5 samples — see note)
offset 16 : i32                  ABSOLUTE offset to the embedded DDS blob
offset 20 : property object      "eCImageResource2": Width, Height, SkipMips, PixelFormat
offset N  : standard DDS file    verbatim, to end of file
```

`offset 16`'s value was confirmed to exactly match the real byte position of the `"DDS "`
signature in all 5 samples (265 in every case, since all 5 are same-sized icons).

### Property encoding

Each scalar property inside the block at offset 20 is a fixed-width TLV:

```
u16      name_len
char[]   name                (e.g. "Width")
u16      type_len
char[]   type                (e.g. "int", "long")
u16      type_tag             (30 = int32, seen for both "int" and "long" so far)
u32      data_len             (4, for int/long)
byte[]   value                (data_len bytes, little-endian)
```

Confirmed present: `Width` (int), `Height` (int), `SkipMips` (long), `PixelFormat4`
(`bTPropertyContainer<enum eCGfxShared::eEColorFormat>`, encoded as a string value —
seen as `"DXT3"` in all 5 samples).

## Why this matters for the pipeline

Because every value slot is fixed-width, **patching `Width`/`Height` in place never shifts
any other byte in the file** — the DDS blob's start offset doesn't move. Rewriting a texture
after an AI upscale is therefore a pure splice:

1. Parse the header (`ximg::parse`), get `dds_offset`.
2. Copy bytes `[0..dds_offset)` from the original file, unchanged.
3. Overwrite the `Width`/`Height` 4-byte slots in that copy with the new dimensions
   (`ximg::replace_dds` does this).
4. Append the new (upscaled) DDS payload after it — any length, since it's the last thing
   in the file.

No re-serialization of the property/reflection system is needed, unlike materials/meshes
(see `content-layer.md`).

## Known gaps

- `SkipMips` and `PixelFormat` are not patched by `replace_dds` yet. If an AI step changes mip
  count or output compression format (e.g. upscaling to a non-DXT3 format), those fields need
  the same find-and-patch treatment as Width/Height.
- All 5 verified samples are 64x64 DXT3 icons with an identical property block size (225
  bytes). This is expected to hold for any image using the same field set and types (TLV slots
  are value-independent in size), but hasn't been confirmed against a texture with different
  dimensions/pixel format/mip count.
- Risen 2's variant of this format has an extra 8-byte prefix handled by `ximghandler.cpp`
  (`iOffset2`) — not implemented here, Risen 1 only for now.
