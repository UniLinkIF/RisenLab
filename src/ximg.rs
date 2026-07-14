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

#[derive(Debug)]
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

/// Locates the byte offset of an `int`/`long` property value, given the property's name.
/// Returns the offset of the first byte of the 4-byte little-endian value.
fn find_property_value_offset(data: &[u8], name: &str) -> Option<usize> {
    let name_idx = find_subslice(data, name.as_bytes())?;
    // layout after the name: [u16 type_len][type bytes]["int"/"long"][u16 tag][u32 datalen][value]
    let after_name = name_idx + name.len();
    let type_len = u16::from_le_bytes([data[after_name], data[after_name + 1]]) as usize;
    let value_offset = after_name + 2 + type_len + 2 + 4;
    Some(value_offset)
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
    let width = i32::from_le_bytes(data[width_off..width_off + 4].try_into().unwrap());
    let height = i32::from_le_bytes(data[height_off..height_off + 4].try_into().unwrap());

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

/// Returns a new `._ximg` byte buffer: same header as `original`, but with Width/Height
/// patched in place and the DDS payload replaced by `new_dds`.
///
/// This does not touch SkipMips or PixelFormat — if an AI upscale step changes mip count
/// or pixel format, those fields need the same find-and-patch treatment (see `find_property_value_offset`).
pub fn replace_dds(original: &[u8], new_width: i32, new_height: i32, new_dds: &[u8]) -> io::Result<Vec<u8>> {
    let info = parse(original)?;
    let mut out = original[..info.dds_offset].to_vec();

    let width_off = find_property_value_offset(&out, "Width").unwrap();
    out[width_off..width_off + 4].copy_from_slice(&new_width.to_le_bytes());
    let height_off = find_property_value_offset(&out, "Height").unwrap();
    out[height_off..height_off + 4].copy_from_slice(&new_height.to_le_bytes());

    out.extend_from_slice(new_dds);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_ximg_data() {
        assert!(parse(b"not an ximg file at all").is_err());
    }
}
