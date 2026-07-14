#!/usr/bin/env python3
"""Minimal DXT3 (BC2) encoder — good enough to prove the ._ximg pipeline round-trip end to end.
Not optimized for quality (no cluster-fit), just correct per the S3TC spec."""
import struct
import sys
import numpy as np
from PIL import Image


def rgb888_to_565(r, g, b):
    r5 = round(r * 31 / 255)
    g6 = round(g * 63 / 255)
    b5 = round(b * 31 / 255)
    return (r5 << 11) | (g6 << 5) | b5


def rgb565_to_888(c565):
    r5 = (c565 >> 11) & 0x1F
    g6 = (c565 >> 5) & 0x3F
    b5 = c565 & 0x1F
    r = (r5 * 255 + 15) // 31
    g = (g6 * 255 + 31) // 63
    b = (b5 * 255 + 15) // 31
    return np.array([r, g, b], dtype=np.float64)


def encode_block_color(block_rgb):
    """block_rgb: (16,3) float array. Returns (color0_565, color1_565, indices[16])."""
    luma = block_rgb @ np.array([0.299, 0.587, 0.114])
    imax, imin = int(np.argmax(luma)), int(np.argmin(luma))
    c0 = rgb888_to_565(*block_rgb[imax].round().astype(int))
    c1 = rgb888_to_565(*block_rgb[imin].round().astype(int))
    if c0 <= c1:
        c0, c1 = (c1, c0) if c1 != c0 else (c0, c0 ^ 1 if c0 != 0xFFFF else c0 - 1)
    if c0 == c1:
        c1 = max(0, c0 - 1)

    p0 = rgb565_to_888(c0)
    p1 = rgb565_to_888(c1)
    p2 = (2 * p0 + p1) / 3
    p3 = (p0 + 2 * p1) / 3
    palette = np.stack([p0, p1, p2, p3])  # index order matches DXT interpolation codes 0..3

    dists = ((block_rgb[:, None, :] - palette[None, :, :]) ** 2).sum(axis=2)
    indices = np.argmin(dists, axis=1)
    return c0, c1, indices


def encode_dxt3(img: Image.Image) -> bytes:
    img = img.convert("RGBA")
    w, h = img.size
    assert w % 4 == 0 and h % 4 == 0, "dimensions must be multiples of 4 for DXT"
    arr = np.asarray(img).astype(np.float64)  # (h, w, 4)

    out = bytearray()
    for by in range(0, h, 4):
        for bx in range(0, w, 4):
            block = arr[by:by + 4, bx:bx + 4, :]  # (4,4,4)
            alpha = block[:, :, 3].reshape(-1)  # 16
            rgb = block[:, :, :3].reshape(-1, 3)  # 16,3

            # --- alpha block: 4-bit per pixel, 2 pixels per byte ---
            a4 = np.clip(np.round(alpha / 255 * 15), 0, 15).astype(np.uint8)
            for i in range(0, 16, 2):
                out.append((a4[i] & 0xF) | ((a4[i + 1] & 0xF) << 4))

            # --- color block ---
            c0, c1, indices = encode_block_color(rgb)
            out += struct.pack('<HH', c0, c1)
            packed = 0
            for i, idx in enumerate(indices):
                packed |= int(idx) << (2 * i)
            out += struct.pack('<I', packed)
    return bytes(out)


def build_dds(width: int, height: int, dxt3_data: bytes) -> bytes:
    """Standard DDS header (124 bytes after magic) + DXT3 compressed body."""
    flags = 0x1 | 0x2 | 0x4 | 0x1000 | 0x80000  # CAPS|HEIGHT|WIDTH|PIXELFORMAT|LINEARSIZE
    pitch = max(1, (width + 3) // 4) * 8  # DXT3 block size = 8 bytes
    header = struct.pack(
        '<4sIIIIIII44x',
        b'DDS ', 124, flags, height, width, pitch, 0, 1
    )
    # pixel format block (32 bytes): size, flags(FOURCC), fourcc, then rgb bit masks (unused for DXT)
    pf = struct.pack('<II4sIIIII', 32, 0x4, b'DXT3', 0, 0, 0, 0, 0)
    caps = struct.pack('<IIIII', 0x1000, 0, 0, 0, 0)
    return header + pf + caps + dxt3_data


if __name__ == '__main__':
    in_path, out_path, scale = sys.argv[1], sys.argv[2], int(sys.argv[3])
    img = Image.open(in_path).convert('RGBA')
    new_size = (img.width * scale, img.height * scale)
    upscaled = img.resize(new_size, Image.LANCZOS)
    compressed = encode_dxt3(upscaled)
    dds_bytes = build_dds(new_size[0], new_size[1], compressed)
    with open(out_path, 'wb') as f:
        f.write(dds_bytes)
    print(f"Wrote {out_path}: {new_size[0]}x{new_size[1]} DXT3, {len(dds_bytes)} bytes")
