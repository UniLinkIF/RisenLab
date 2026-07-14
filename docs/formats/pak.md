# `.pak` / `.pXX` container format

Source: Nico Bendlin, `RisenPAK.txt` (2009-2011), bundled with his `RisenPAK.exe`/`Risenaut.exe`
tools (MIT-style license — free use/modify/distribute with attribution). Independently
re-implemented in `src/pak.rs`.

Verified against real files from a licensed Risen 1 install:

- `library.pak` (112,904 bytes) → header `VolumeSize` = 112,904 (exact match), 3 files
  extracted with correct names and sizes (`WeatherStates.g3ws`, `Risen Credits.txt`, `g3.svm`).
- `materials.pak` (2,100,437 bytes) → 516 files, all real Risen material names
  (`Static_Bark_*`, `ST_Composite_*`, `Special_Water_*`, ...), all sizes correct.
- Full unpack → repack round trip of `library.pak` reproduced identical `root_offset` and
  `volume_size` to the original.

## Layout

All integers little-endian. Offsets absolute (from start of file).

```
+---------------+
|  file header  |   48 bytes, fixed size
+---------------+
|   file data   |
+---------------+
|  entry table  |   directory tree, at header.RootOffset
+---------------+
```

### Header (48 bytes)

| Field       | Type | Notes |
|-------------|------|-------|
| Version     | u32  | seen: 1 |
| Product     | u32  | `0x30563347` = ASCII `"G3V0"` — same tag family as Gothic 3 |
| Revision    | u32  | seen: 0 |
| Encryption  | u32  | 0 = none (only value seen) |
| Compression | u32  | 0 = none, 1 = auto, 2 = zlib (whole-volume hint, not authoritative — see below) |
| Reserved    | u32  | 0 |
| DataOffset  | u64  | always 48 = `sizeof(header)` |
| RootOffset  | u64  | absolute offset of the root `Directory` entry |
| VolumeSize  | u64  | == exact file size |

### Directory entry (recursive)

```
u32      NameLength
char[]   Name              (NameLength bytes, + 1 null terminator if NameLength != 0)
u64      TimeCreated        FILETIME
u64      TimeLastAccessed   FILETIME
u64      TimeLastModified   FILETIME
u32      FileAttributes
u32      Count
for Count entries:
    u32  Attributes         <- discriminator, read BEFORE the entry itself
    if Attributes & 0x10 (DIRECTORY):
        Directory           <- recurse
    else:
        File
```

### File entry

```
u32      NameLength
char[]   Name
u64      DataOffset
u64      TimeCreated / TimeLastAccessed / TimeLastModified
u32      FileAttributes
u32      Encryption
u32      Compression        <- per-file, this is the one that actually matters
u32      DataSize           <- size on disk (compressed, if Compression == 2)
u32      FileSize           <- size after decompression
```

Compression is decided **per file**, not globally — `materials.pak`'s header says
`Compression=1` ("auto") but every one of its 516 entries ended up uncompressed (small text-like
`.xmat` files apparently weren't worth it). The zlib decompression path (`flate2`) is
implemented in `pak.rs` but not yet exercised against a real compressed entry — likely to occur
in `images.pak` or `sounds.pak`, not yet tested.

### Attributes bit relevant to patch volumes

`RisenPakAttribute_Deleted = 0x8000`. `RisenPAK.exe`'s own documented convention: create a
0-byte file named `<name>.deleted` next to real files when building a volume, and the packer
marks the resulting entry as deleted instead of writing data. When the game merges a patch
volume (`.p0x`) over a base `.pak`, a deleted-marked entry removes that file from the merged
view. See `docs/p0x-patches.md`.

## Known gaps

- Write path here always builds a flat, single-directory archive (matches what `materials.pak`
  looks like) — subfolder support in the writer isn't implemented yet.
- Compression on write is not implemented (`write_archive_from_dir` always writes
  `Compression::None`); needed before patch volumes for texture-heavy content stay a reasonable
  size.
- Exact patch-volume merge/priority rule (does `.p02` always win over `.p01` for the same
  filename, or is it order-of-mount?) is inferred from `Rimy3D`'s own search-path priority
  logic, not confirmed against the actual game engine. Needs an empirical test.
