//! Pixel-data codec for the DDS payload embedded in a `._ximg` file: decode to plain RGBA8
//! (so any ordinary image editor / AI tool can open it as a PNG) and encode back.
//!
//! `ddsfile` handles the DDS container (header, format, layout) — it does not touch pixel
//! bytes. Block-compressed formats (DXT1/3/5) are decoded/encoded with `texpresso` (a pure
//! Rust, from-scratch S3TC/BCn implementation, MIT-licensed — not the GPL `mimicry` codec).
//! Uncompressed formats are handled here directly via channel-mask unpacking.

use anyhow::{anyhow, bail, Result};
use ddsfile::{D3DFormat, DataFormat, Dds, NewD3dParams, PixelFormat, PixelFormatFlags};
use texpresso::{Format as BcFormat, Params as BcParams};

/// A decoded texture: straight top-level mip only, RGBA8, row-major, no padding.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn bc_format_for(fmt: D3DFormat) -> Option<BcFormat> {
    match fmt {
        D3DFormat::DXT1 => Some(BcFormat::Bc1),
        D3DFormat::DXT2 | D3DFormat::DXT3 => Some(BcFormat::Bc2),
        D3DFormat::DXT4 | D3DFormat::DXT5 => Some(BcFormat::Bc3),
        _ => None,
    }
}

/// Determines the D3D pixel format of a parsed DDS, working around a real `ddsfile` 0.6.0
/// limitation: `PixelFormat::read` only populates `r_bit_mask` when the `RGB` flag is set,
/// never for `LUMINANCE`-only formats — so `get_d3d_format()` can't recognize plain `L8`
/// textures even though the header is unambiguous (`LUMINANCE` flag + 8 bits, no alpha).
/// Encountered on real assets (e.g. `Special_Detail_*` blend masks).
pub fn resolve_format(dds: &Dds) -> Option<D3DFormat> {
    if let Some(fmt) = dds.get_d3d_format() {
        return Some(fmt);
    }
    let spf = &dds.header.spf;
    let is_luminance_only = spf.flags.contains(PixelFormatFlags::LUMINANCE)
        && !spf.flags.contains(PixelFormatFlags::ALPHA)
        && !spf.flags.contains(PixelFormatFlags::ALPHA_PIXELS);
    if is_luminance_only && spf.rgb_bit_count == Some(8) {
        return Some(D3DFormat::L8);
    }
    None
}

/// For formats where exactly one channel carries data (`L8`'s luminance, `A8`'s alpha), the
/// mask of that one active channel — used to view/edit them as an ordinary opaque grayscale
/// photo (R=G=B=value, A=255) instead of e.g. a "black, mostly-transparent" image for `A8`.
fn single_channel_mask(fmt: D3DFormat) -> Option<u32> {
    match fmt {
        D3DFormat::L8 => fmt.r_bit_mask(),
        D3DFormat::A8 => fmt.a_bit_mask(),
        _ => None,
    }
}

fn bytes_per_pixel(fmt: D3DFormat) -> Result<usize> {
    let bpp = fmt
        .get_bits_per_pixel()
        .ok_or_else(|| anyhow!("format {:?} has no fixed bits-per-pixel (compressed?)", fmt))?;
    if bpp % 8 != 0 {
        bail!("format {:?} has a non-byte-aligned bit depth ({} bits) — not supported", fmt, bpp);
    }
    Ok((bpp / 8) as usize)
}

fn extract_channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let bits = mask.count_ones();
    let value = (pixel & mask) >> shift;
    // Scale an n-bit value up to 8 bits.
    if bits >= 8 {
        (value >> (bits - 8)) as u8
    } else {
        ((value << (8 - bits)) | (value >> (bits.saturating_sub(8).min(bits)))) as u8
    }
}

fn place_channel(value: u8, mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let bits = mask.count_ones();
    let scaled = if bits >= 8 {
        (value as u32) << (bits - 8)
    } else {
        (value as u32) >> (8 - bits)
    };
    (scaled << shift) & mask
}

/// Unpacks an uncompressed format (any byte-aligned bit depth: 8/16/24/32bpp) into RGBA8,
/// using the format's own channel bitmasks so byte order (e.g. BGRA vs RGBA) and single-
/// channel formats (L8, A8) are all handled generically.
fn unpack_uncompressed(data: &[u8], width: u32, height: u32, fmt: D3DFormat) -> Result<Vec<u8>> {
    let bpp = bytes_per_pixel(fmt)?;
    let pixel_count = (width as usize) * (height as usize);
    let expected = pixel_count * bpp;
    if data.len() < expected {
        bail!(
            "uncompressed DDS data too short: got {} bytes, need {}",
            data.len(),
            expected
        );
    }

    let mut out = Vec::with_capacity(pixel_count * 4);
    if let Some(value_mask) = single_channel_mask(fmt) {
        for chunk in data[..expected].chunks_exact(bpp) {
            let mut buf = [0u8; 4];
            buf[..bpp].copy_from_slice(chunk);
            let pixel = u32::from_le_bytes(buf);
            let v = extract_channel(pixel, value_mask);
            out.extend_from_slice(&[v, v, v, 255]);
        }
    } else {
        let r_mask = fmt.r_bit_mask().unwrap_or(0);
        let g_mask = fmt.g_bit_mask().unwrap_or(0);
        let b_mask = fmt.b_bit_mask().unwrap_or(0);
        let a_mask = fmt.a_bit_mask();
        for chunk in data[..expected].chunks_exact(bpp) {
            let mut buf = [0u8; 4];
            buf[..bpp].copy_from_slice(chunk);
            let pixel = u32::from_le_bytes(buf);
            out.push(extract_channel(pixel, r_mask));
            out.push(extract_channel(pixel, g_mask));
            out.push(extract_channel(pixel, b_mask));
            out.push(a_mask.map(|m| extract_channel(pixel, m)).unwrap_or(255));
        }
    }
    Ok(out)
}

/// Packs RGBA8 into an uncompressed format matching the target's channel bitmasks and bit
/// depth (single-channel formats take their value from the R channel, matching what
/// `unpack_uncompressed` wrote there).
fn pack_uncompressed(rgba: &[u8], fmt: D3DFormat) -> Result<Vec<u8>> {
    let bpp = bytes_per_pixel(fmt)?;
    let mut out = Vec::with_capacity(rgba.len() / 4 * bpp);

    if let Some(value_mask) = single_channel_mask(fmt) {
        for px in rgba.chunks_exact(4) {
            let pixel = place_channel(px[0], value_mask);
            out.extend_from_slice(&pixel.to_le_bytes()[..bpp]);
        }
    } else {
        let r_mask = fmt.r_bit_mask().unwrap_or(0);
        let g_mask = fmt.g_bit_mask().unwrap_or(0);
        let b_mask = fmt.b_bit_mask().unwrap_or(0);
        let a_mask = fmt.a_bit_mask();
        for px in rgba.chunks_exact(4) {
            let mut pixel = place_channel(px[0], r_mask) | place_channel(px[1], g_mask) | place_channel(px[2], b_mask);
            if let Some(m) = a_mask {
                pixel |= place_channel(px[3], m);
            }
            out.extend_from_slice(&pixel.to_le_bytes()[..bpp]);
        }
    }
    Ok(out)
}

/// Decodes the top-level mip of an embedded DDS blob to plain RGBA8.
pub fn decode(dds_bytes: &[u8]) -> Result<DecodedImage> {
    let dds = Dds::read(dds_bytes)?;
    let width = dds.get_width();
    let height = dds.get_height();
    let format = resolve_format(&dds).ok_or_else(|| anyhow!("unrecognized or non-D3D DDS pixel format"))?;

    // Not `dds.get_data(0)`/`get_main_texture_size()` — both re-derive the format from the
    // header internally and hit the same LUMINANCE-recognition gap `resolve_format` works
    // around (see its doc comment). Compute the top-level mip size from the format we
    // already resolved instead, and slice the public `data` field directly.
    let main_size = if let Some(bc) = bc_format_for(format) {
        bc.compressed_size(width as usize, height as usize)
    } else {
        (width as usize) * (height as usize) * bytes_per_pixel(format)?
    };
    if dds.data.len() < main_size {
        bail!(
            "DDS data too short for top-level mip: got {} bytes, need {main_size}",
            dds.data.len()
        );
    }
    let level0 = &dds.data[..main_size];

    let rgba = if let Some(bc) = bc_format_for(format) {
        let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
        bc.decompress(level0, width as usize, height as usize, &mut out);
        out
    } else {
        unpack_uncompressed(level0, width, height, format)?
    };

    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}

/// Encodes RGBA8 pixel data into a standalone single-mip DDS file using the given D3D pixel
/// format (normally the original texture's own format, so the round trip doesn't change
/// what the engine expects to load).
pub fn encode(width: u32, height: u32, rgba: &[u8], format: D3DFormat) -> Result<Vec<u8>> {
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        bail!(
            "rgba buffer length {} does not match {}x{} RGBA8",
            rgba.len(),
            width,
            height
        );
    }

    let mut dds = Dds::new_d3d(NewD3dParams {
        height,
        width,
        depth: None,
        format,
        mipmap_levels: Some(1),
        caps2: None,
    })?;

    // `ddsfile`'s generic `From<D3DFormat> for PixelFormat` always tags single-channel
    // formats as `RGB` (+ a redundant `ALPHA_PIXELS` for A8), never the spec-correct
    // `LUMINANCE`/`ALPHA`-only flags real Risen assets use — which then fails to
    // round-trip back through `get_d3d_format()` on a later read. Overwrite the header's
    // pixel format directly for these two, *before* any call that needs to resolve the
    // format back from the header (like `get_mut_data`), so it stays self-consistent.
    match format {
        D3DFormat::L8 => {
            dds.header.spf = PixelFormat {
                size: 32,
                flags: PixelFormatFlags::LUMINANCE,
                fourcc: None,
                rgb_bit_count: Some(8),
                r_bit_mask: Some(0xff),
                g_bit_mask: None,
                b_bit_mask: None,
                a_bit_mask: None,
            };
        }
        D3DFormat::A8 => {
            dds.header.spf = PixelFormat {
                size: 32,
                flags: PixelFormatFlags::ALPHA,
                fourcc: None,
                rgb_bit_count: None,
                r_bit_mask: None,
                g_bit_mask: None,
                b_bit_mask: None,
                a_bit_mask: Some(0xff),
            };
        }
        _ => {}
    }

    let body = if let Some(bc) = bc_format_for(format) {
        let mut out = vec![0u8; bc.compressed_size(width as usize, height as usize)];
        bc.compress(rgba, width as usize, height as usize, BcParams::default(), &mut out);
        out
    } else {
        pack_uncompressed(rgba, format)?
    };

    // Not `dds.get_mut_data(0)` — that accessor re-derives the format from the header to
    // compute the valid data range, which fails the same way `get_d3d_format()` does for
    // LUMINANCE-flagged formats (see above). `dds.data` was already sized correctly by
    // `Dds::new_d3d` from the `format` parameter directly, so write to it as-is.
    if body.len() > dds.data.len() {
        bail!(
            "encoded body ({} bytes) exceeds allocated DDS data buffer ({} bytes)",
            body.len(),
            dds.data.len()
        );
    }
    dds.data[..body.len()].copy_from_slice(&body);

    let mut out = Vec::new();
    dds.write(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkerboard(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let on = (x / 4 + y / 4) % 2 == 0;
                if on {
                    out.extend_from_slice(&[255, 0, 0, 255]);
                } else {
                    out.extend_from_slice(&[0, 0, 255, 128]);
                }
            }
        }
        out
    }

    #[test]
    fn dxt5_round_trip_preserves_dimensions_and_is_visually_close() {
        let (w, h) = (16, 16);
        let original = checkerboard(w, h);
        let dds_bytes = encode(w, h, &original, D3DFormat::DXT5).unwrap();
        let decoded = decode(&dds_bytes).unwrap();
        assert_eq!(decoded.width, w);
        assert_eq!(decoded.height, h);
        assert_eq!(decoded.rgba.len(), original.len());

        // Block compression is lossy — assert closeness, not exact equality.
        let mut max_diff = 0i32;
        for (a, b) in original.iter().zip(decoded.rgba.iter()) {
            max_diff = max_diff.max((*a as i32 - *b as i32).abs());
        }
        assert!(max_diff <= 40, "max channel diff too large: {max_diff}");
    }

    #[test]
    fn dxt1_round_trip_preserves_dimensions() {
        let (w, h) = (8, 8);
        let original = checkerboard(w, h);
        let dds_bytes = encode(w, h, &original, D3DFormat::DXT1).unwrap();
        let decoded = decode(&dds_bytes).unwrap();
        assert_eq!((decoded.width, decoded.height), (w, h));
    }

    #[test]
    fn a8r8g8b8_round_trip_is_lossless() {
        let (w, h) = (4, 4);
        let original = checkerboard(w, h);
        let dds_bytes = encode(w, h, &original, D3DFormat::A8R8G8B8).unwrap();
        let decoded = decode(&dds_bytes).unwrap();
        assert_eq!(decoded.rgba, original, "uncompressed format must round-trip exactly");
    }

    #[test]
    fn a8b8g8r8_round_trip_is_lossless() {
        let (w, h) = (4, 4);
        let original = checkerboard(w, h);
        let dds_bytes = encode(w, h, &original, D3DFormat::A8B8G8R8).unwrap();
        let decoded = decode(&dds_bytes).unwrap();
        assert_eq!(decoded.rgba, original);
    }

    /// Grayscale gradient — a single value per pixel, replicated into R/G/B by
    /// `unpack_uncompressed` so single-channel formats view as an ordinary opaque photo.
    fn grayscale_gradient(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let v = ((x + y) * 255 / (width + height).max(1)) as u8;
                out.extend_from_slice(&[v, v, v, 255]);
            }
        }
        out
    }

    /// Regression test for a real bug found on an actual game texture
    /// (`Special_Detail_03_Diffuse_01._ximg`): `ddsfile::get_d3d_format()` can't recognize
    /// `L8` (it only populates channel bitmasks for the `RGB` flag, never `LUMINANCE`), and a
    /// full encode→decode→encode round trip must still resolve the format correctly on the
    /// second pass, not just decode a file that happened to come from the real game.
    #[test]
    fn l8_round_trips_through_two_full_encode_decode_cycles() {
        let (w, h) = (8, 8);
        let original = grayscale_gradient(w, h);

        let dds_bytes = encode(w, h, &original, D3DFormat::L8).unwrap();
        let decoded = decode(&dds_bytes).unwrap();
        assert_eq!(decoded.rgba, original, "L8 is lossless (no sub-byte packing)");

        // Second pass: re-encode what we just decoded and decode again — this is what
        // `resolve_format`'s LUMINANCE fallback exists for for (get_d3d_format() alone
        // can't recognize our own re-written L8 header either).
        let dds = ddsfile::Dds::read(&dds_bytes[..]).unwrap();
        assert!(dds.get_d3d_format().is_none(), "ddsfile itself still can't recognize L8 — that's the known gap resolve_format works around");
        let format = resolve_format(&dds).expect("resolve_format must recognize our own L8 output");
        assert_eq!(format, D3DFormat::L8);

        let dds_bytes_2 = encode(w, h, &decoded.rgba, format).unwrap();
        let decoded_2 = decode(&dds_bytes_2).unwrap();
        assert_eq!(decoded_2.rgba, original);
    }

    /// Same real-world class of bug as L8 (`Nat_Veg_Herbs_02_Distance_02._ximg`, a
    /// single-channel alpha/blend mask): the naive uncompressed unpacker assumed every
    /// uncompressed format was 4 bytes/pixel, so an 8bpp A8 texture failed with
    /// "data too short" (it asked for 4x the real data size).
    #[test]
    fn a8_round_trip_is_lossless_and_viewable_as_grayscale() {
        let (w, h) = (8, 8);
        let original = grayscale_gradient(w, h);

        let dds_bytes = encode(w, h, &original, D3DFormat::A8).unwrap();
        let decoded = decode(&dds_bytes).unwrap();
        assert_eq!(decoded.rgba, original);

        let dds = ddsfile::Dds::read(&dds_bytes[..]).unwrap();
        let format = dds.get_d3d_format().expect("A8 with the ALPHA-only flag must round-trip via ddsfile directly");
        assert_eq!(format, D3DFormat::A8);
    }
}
