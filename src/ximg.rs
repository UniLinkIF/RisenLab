//! Risen 1/2 `._ximg` texture format.
//!
//! Reverse-engineered from `QImageIOPlugin/ximghandler.cpp` (rmtools, GPL-3.0, used here only
//! as a reference to confirm field offsets — this module is an independent implementation)
//! and verified byte-for-byte against 5 real `._ximg` files extracted from a licensed Risen 1
//! install (all successfully decoded to valid, viewable 64x64 DDS/PNG images).
//!
//! Layout (little-endian):
//! ```text
//! offset 0  : "GR01IM04"            8 bytes, magic
//! offset 8  : i32 = 40              resource header size (constant)
//! offset 12 : i32                   property block size
//! offset 16 : i32                   absolute offset to the embedded DDS blob
//! offset 20 : eCImageResource2      property object: Width/Height/SkipMips/PixelFormat (TLV)
//! offset N  : standard DDS file     to end of file
//! ```
//!
//! Each scalar property is encoded as:
//! `[u16 name_len][name][u16 type_len]["int"|"long"][u16 type_tag][u32 data_len][value bytes]`
//! Because these are fixed-width slots, patching an int value in place never shifts any
//! other offset in the file — which is what makes rewriting Width/Height after an AI
//! upscale a pure byte-splice, no re-serialization of the property system required.

use std::io;

const MAGIC: &[u8; 8] = b"GR01IM04";

// Fields are shown to the user via `{info:#?}` (the whole point of `ximg-info`), but rustc's
// dead-code lint doesn't count Debug-derive formatting as a "read", hence the blanket allow.
#[derive(Debug)]
#[allow(dead_code)]
pub struct XimgInfo {
    pub header_const: i32,
    pub property_block_size: i32,
    pub dds_offset: usize,
    pub width: i32,
    pub height: i32,
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// A located property value slot: `[u16 datalen][value bytes]` at `datalen_offset`.
struct PropertySlot {
    datalen_offset: usize,
    value_offset: usize,
    data_len: usize,
}

/// Locates a property's value slot by name. Layout after the name:
/// `[u16 type_len][type bytes]["int"/"long"/enum-container name][u16 tag][u32 datalen][value]`
fn find_property_slot(data: &[u8], name: &str) -> Option<PropertySlot> {
    let name_idx = find_subslice(data, name.as_bytes())?;
    let after_name = name_idx + name.len();
    let type_len = u16::from_le_bytes(data.get(after_name..after_name + 2)?.try_into().ok()?) as usize;
    let after_type = after_name + 2 + type_len;
    let datalen_offset = after_type + 2; // skip the 2-byte tag
    let data_len = u32::from_le_bytes(data.get(datalen_offset..datalen_offset + 4)?.try_into().ok()?) as usize;
    Some(PropertySlot {
        datalen_offset,
        value_offset: datalen_offset + 4,
        data_len,
    })
}

/// Locates the byte offset of an `int`/`long` property value (always a fixed 4-byte slot —
/// `Width`, `Height`, `SkipMips`).
fn find_property_value_offset(data: &[u8], name: &str) -> Option<usize> {
    find_property_slot(data, name).map(|s| s.value_offset)
}

/// `PixelFormat`'s value is `[2-byte marker][ASCII format name, no terminator]` (e.g. the
/// marker byte pair followed by "DXT3"). Returns the format name as a string.
pub fn read_pixel_format(data: &[u8]) -> io::Result<String> {
    let slot = find_property_slot(data, "PixelFormat")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "PixelFormat property not found"))?;
    let value = &data[slot.value_offset..slot.value_offset + slot.data_len];
    if value.len() < 2 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "PixelFormat value too short"));
    }
    Ok(String::from_utf8_lossy(&value[2..]).into_owned())
}

/// Overwrites the `PixelFormat` property's string value in place, growing/shrinking the
/// buffer if the new name is a different length than the old one, and keeping the 2-byte
/// marker that precedes it unchanged. Returns the byte-count delta (new size - old size),
/// which the caller must add to `property_block_size` (offset 12) and `dds_offset` (offset
/// 16) since everything after this property shifts by that amount.
fn patch_pixel_format(data: &mut Vec<u8>, new_format: &str) -> io::Result<isize> {
    let slot = find_property_slot(data, "PixelFormat")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "PixelFormat property not found"))?;
    if slot.data_len < 2 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "PixelFormat value too short"));
    }
    let marker = [data[slot.value_offset], data[slot.value_offset + 1]];
    let mut new_value = marker.to_vec();
    new_value.extend_from_slice(new_format.as_bytes());
    let delta = new_value.len() as isize - slot.data_len as isize;

    data.splice(slot.value_offset..slot.value_offset + slot.data_len, new_value);
    let new_data_len = (slot.data_len as isize + delta) as u32;
    data[slot.datalen_offset..slot.datalen_offset + 4].copy_from_slice(&new_data_len.to_le_bytes());
    Ok(delta)
}

pub fn parse(data: &[u8]) -> io::Result<XimgInfo> {
    if data.len() < 20 || &data[0..8] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a Risen 1/2 GR01IM04 ._ximg file",
        ));
    }
    let header_const = i32::from_le_bytes(data[8..12].try_into().unwrap());
    let property_block_size = i32::from_le_bytes(data[12..16].try_into().unwrap());
    let dds_offset = i32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;

    if data.len() < dds_offset + 4 || &data[dds_offset..dds_offset + 4] != b"DDS " {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "dds_offset field does not point at a DDS signature",
        ));
    }

    let width_off = find_property_value_offset(data, "Width")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Width property not found"))?;
    let height_off = find_property_value_offset(data, "Height")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Height property not found"))?;
    // Checked reads: `width_off`/`height_off` are real offsets into the property block, but a
    // truncated/corrupted file can still place one near enough to the end of the buffer that
    // there aren't 4 more bytes to read — an unchecked slice index there panics the whole CLI
    // instead of surfacing a clean parse error.
    let width = i32::from_le_bytes(
        data.get(width_off..width_off + 4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Width value truncated"))?
            .try_into()
            .unwrap(),
    );
    let height = i32::from_le_bytes(
        data.get(height_off..height_off + 4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Height value truncated"))?
            .try_into()
            .unwrap(),
    );

    Ok(XimgInfo {
        header_const,
        property_block_size,
        dds_offset,
        width,
        height,
    })
}

/// Extracts the embedded standard DDS payload — openable by any DDS-capable image tool.
pub fn extract_dds(data: &[u8]) -> io::Result<&[u8]> {
    let info = parse(data)?;
    Ok(&data[info.dds_offset..])
}

/// What to change when splicing a new DDS payload into a `._ximg`. `pixel_format` is the only
/// field that can change the file's total layout (Width/Height/SkipMips are always 4-byte
/// slots patched in place; a pixel format name of a different length shifts everything after
/// it, including where the DDS payload starts).
#[derive(Debug, Default)]
pub struct ReplaceOptions<'a> {
    pub width: i32,
    pub height: i32,
    pub skip_mips: Option<i32>,
    pub pixel_format: Option<&'a str>,
}

/// Returns a new `._ximg` byte buffer: same header as `original` but with Width/Height (and
/// optionally SkipMips/PixelFormat) patched, and the DDS payload replaced by `new_dds`.
pub fn replace_dds(original: &[u8], opts: ReplaceOptions, new_dds: &[u8]) -> io::Result<Vec<u8>> {
    let info = parse(original)?;
    let mut out = original[..info.dds_offset].to_vec();

    let width_off = find_property_value_offset(&out, "Width").unwrap();
    out[width_off..width_off + 4].copy_from_slice(&opts.width.to_le_bytes());
    let height_off = find_property_value_offset(&out, "Height").unwrap();
    out[height_off..height_off + 4].copy_from_slice(&opts.height.to_le_bytes());

    if let Some(mips) = opts.skip_mips {
        let off = find_property_value_offset(&out, "SkipMips").ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "SkipMips property not found")
        })?;
        out[off..off + 4].copy_from_slice(&mips.to_le_bytes());
    }

    let mut shift: isize = 0;
    if let Some(fmt) = opts.pixel_format {
        shift = patch_pixel_format(&mut out, fmt)?;
    }

    if shift != 0 {
        // Best-effort: property_block_size's exact accounting relative to the preceding
        // resource-wrapper bytes isn't fully nailed down (see docs/formats/ximg.md), but
        // shifting it by the same delta keeps it internally consistent. dds_offset is the
        // field that actually matters for locating pixel data, and this keeps it exact.
        let new_prop_block_size = (info.property_block_size as isize + shift) as i32;
        out[12..16].copy_from_slice(&new_prop_block_size.to_le_bytes());
        let new_dds_offset = (info.dds_offset as isize + shift) as i32;
        out[16..20].copy_from_slice(&new_dds_offset.to_le_bytes());
    }

    out.extend_from_slice(new_dds);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_prop(buf: &mut Vec<u8>, name: &str, type_name: &str, value: &[u8]) {
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&(type_name.len() as u16).to_le_bytes());
        buf.extend_from_slice(type_name.as_bytes());
        buf.extend_from_slice(&30u16.to_le_bytes()); // tag
        buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
        buf.extend_from_slice(value);
    }

    /// Builds a minimal synthetic `._ximg`-shaped buffer matching the documented layout, so
    /// tests don't depend on committing real (licensed) game assets to the repo.
    fn synthetic_ximg(width: i32, height: i32, pixel_format: &str) -> Vec<u8> {
        let mut props = Vec::new();
        push_prop(&mut props, "Width", "int", &width.to_le_bytes());
        push_prop(&mut props, "Height", "int", &height.to_le_bytes());
        push_prop(&mut props, "SkipMips", "long", &0i32.to_le_bytes());
        let mut fmt_value = vec![0xC9, 0x00];
        fmt_value.extend_from_slice(pixel_format.as_bytes());
        push_prop(&mut props, "PixelFormat", "bTPropertyContainer<enum eCGfxShared::eEColorFormat>", &fmt_value);

        let dds_offset = 20 + props.len();
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&40i32.to_le_bytes());
        buf.extend_from_slice(&(props.len() as i32).to_le_bytes());
        buf.extend_from_slice(&(dds_offset as i32).to_le_bytes());
        buf.extend_from_slice(&props);
        buf.extend_from_slice(b"DDS "); // fake payload, just needs the signature
        buf.extend_from_slice(&[0u8; 16]);
        buf
    }

    #[test]
    fn rejects_non_ximg_data() {
        assert!(parse(b"not an ximg file at all").is_err());
    }

    /// A real (if unlikely) failure mode: `find_property_slot` searches the WHOLE buffer for
    /// the property name (not just the property block before `dds_offset`), so a truncated or
    /// adversarial file can still yield a "Width" match near the very end — this must return a
    /// clean parse error, not panic on an out-of-bounds slice index reading the 4-byte value.
    #[test]
    fn truncated_width_value_is_a_parse_error_not_a_panic() {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&40i32.to_le_bytes()); // header_const
        buf.extend_from_slice(&0i32.to_le_bytes()); // property_block_size (unchecked by parse)
        buf.extend_from_slice(&20i32.to_le_bytes()); // dds_offset points right after the header
        buf.extend_from_slice(b"DDS "); // satisfies the dds_offset signature check
        buf.extend_from_slice(&[0u8; 12]); // padding so the DDS "payload" isn't suspiciously tiny
        // Height is complete (parse() looks it up too, before either value is read) — only
        // Width, pushed last, has its declared 4-byte value cut short by 2 bytes.
        // find_property_slot fully parses name/type/tag/datalen for both (all present), so
        // both offsets are found; only Width's final value read is short.
        push_prop(&mut buf, "Height", "int", &64i32.to_le_bytes());
        push_prop(&mut buf, "Width", "int", &1234i32.to_le_bytes());
        let short = buf.len() - 2;
        buf.truncate(short);

        let err = parse(&buf).expect_err("truncated Width value must not panic");
        assert!(err.to_string().contains("Width"), "error should name the field: {err}");
    }

    #[test]
    fn parses_synthetic_header() {
        let data = synthetic_ximg(64, 64, "DXT3");
        let info = parse(&data).unwrap();
        assert_eq!(info.width, 64);
        assert_eq!(info.height, 64);
        assert_eq!(read_pixel_format(&data).unwrap(), "DXT3");
    }

    #[test]
    fn replace_dds_same_length_format_does_not_shift_offset() {
        let data = synthetic_ximg(64, 64, "DXT3");
        let before = parse(&data).unwrap();
        let out = replace_dds(
            &data,
            ReplaceOptions { width: 256, height: 256, skip_mips: None, pixel_format: Some("DXT5") },
            b"DDS \x00\x00\x00\x00fake-pixels",
        ).unwrap();
        let after = parse(&out).unwrap();
        assert_eq!(after.width, 256);
        assert_eq!(after.height, 256);
        assert_eq!(after.dds_offset, before.dds_offset, "same-length format swap must not move dds_offset");
        assert_eq!(read_pixel_format(&out).unwrap(), "DXT5");
    }

    #[test]
    fn replace_dds_longer_format_shifts_offset_correctly() {
        let data = synthetic_ximg(64, 64, "DXT3");
        let before = parse(&data).unwrap();
        let new_dds = b"DDS \x00\x00\x00\x00fake-pixels";
        let out = replace_dds(
            &data,
            ReplaceOptions { width: 128, height: 128, skip_mips: Some(1), pixel_format: Some("UNCOMPRESSED") },
            new_dds,
        ).unwrap();
        let after = parse(&out).unwrap();
        let expected_shift = "UNCOMPRESSED".len() as isize - "DXT3".len() as isize;
        assert_eq!(after.dds_offset as isize, before.dds_offset as isize + expected_shift);
        assert_eq!(read_pixel_format(&out).unwrap(), "UNCOMPRESSED");
        // and the DDS payload itself must still be exactly where dds_offset claims
        assert_eq!(&out[after.dds_offset..after.dds_offset + 4], b"DDS ");
        assert_eq!(&out[after.dds_offset..], new_dds);
    }
}
