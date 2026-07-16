//! Risen 1 `._xmac` actor file — just the **node/skeleton hierarchy** section (not the mesh,
//! skin, or material sections, which are already handled by shelling out to `mimicry-helper`
//! for OBJ conversion — see `batch::actor_to_obj_from_archive`). This is an independent,
//! from-scratch Rust port of the node-reading logic in the vendored, working C++ reference
//! (`mimicry-helper/vendor/mimicry-source/Mimicry/mi_xmacreader.cpp`, `ESection_Nodes`),
//! kept deliberately narrow: just enough to build a bone hierarchy to animate with a parsed
//! `.xmot` motion clip (see `xmot.rs`), without needing a `mimicry-helper.exe` round trip.
//!
//! Layout, matching the C++ reference exactly:
//! - byte 136: `u32` extra length; total section-table end offset = that value + 140
//! - byte 140: `"XAC"` magic (3 bytes, no null)
//! - byte 146: `bool` (1 byte) — if set, every multi-byte field from here on is big-endian
//!   instead of little-endian (confirmed real: some real Risen 1 actors do set this, not just
//!   a theoretical case — the reader flips endianness for the rest of the file the moment it
//!   sees this byte, matching the real `mCIOStreamBinary::SetInvertEndianness` call site)
//! - byte 148: first section header, `{sectionId: u32, sizeInBytes: u32, version: u32}` (12
//!   bytes; `sizeInBytes` excludes this header), sections repeat until the end offset
//! - the nodes section (id 11): `nodeCount: u32`, 4 bytes skip, then `nodeCount` fixed-shape
//!   records: `quat(16) + skip(16) + pos(12) + skip(32) + parentIndex(u32) + skip(76) +
//!   [u32 nameLen][name bytes]`

use anyhow::{bail, Result};
use serde::Serialize;

const SECTION_NODES: u32 = 11;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonNode {
    pub name: String,
    /// Index into the same skeleton's node list, or `None` for a root node.
    pub parent_index: Option<usize>,
    /// Bind-pose local rotation, `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Bind-pose local position, `[x, y, z]`.
    pub position: [f32; 3],
}

fn read_u32(data: &[u8], off: usize, big_endian: bool) -> Result<u32> {
    let bytes: [u8; 4] = data
        .get(off..off + 4)
        .ok_or_else(|| anyhow::anyhow!("xmac: unexpected end of file at offset {off}"))?
        .try_into()
        .unwrap();
    Ok(if big_endian { u32::from_be_bytes(bytes) } else { u32::from_le_bytes(bytes) })
}

fn read_f32(data: &[u8], off: usize, big_endian: bool) -> Result<f32> {
    Ok(f32::from_bits(read_u32(data, off, big_endian)?))
}

fn read_string(data: &[u8], off: usize, len: usize) -> Result<String> {
    let bytes = data
        .get(off..off + len)
        .ok_or_else(|| anyhow::anyhow!("xmac: unexpected end of file reading a {len}-byte string at {off}"))?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// Parses just the skeleton (bone name + parent index + bind-pose local transform) out of a
/// decompressed `._xmac` actor file, in the same order the file stores them (a node's parent
/// always has a smaller index than the node itself, matching the C++ reference's assumption).
pub fn parse_skeleton(data: &[u8]) -> Result<Vec<SkeletonNode>> {
    if data.len() < 150 {
        bail!("xmac: file too short to be a valid ._xmac actor ({} bytes)", data.len());
    }
    // The real reader reads this field, and checks the "XAC" magic, before it even looks at
    // the endianness flag at byte 146 — so this one field is always little-endian regardless.
    let end_section_offset = read_u32(data, 136, false)? as usize + 140;
    if read_string(data, 140, 3)? != "XAC" {
        bail!("xmac: missing 'XAC' magic at offset 140");
    }
    let big_endian = data[146] != 0;

    let mut nodes = Vec::new();
    let mut next_section = 148usize;
    while next_section < end_section_offset {
        let section_id = read_u32(data, next_section, big_endian)?;
        let section_size = read_u32(data, next_section + 4, big_endian)? as usize + 12;
        let section_start = next_section;
        next_section += section_size;

        if section_id != SECTION_NODES {
            continue;
        }

        // 12-byte section header (id, size, version) + u32 nodeCount + 4-byte skip.
        let node_count = read_u32(data, section_start + 12, big_endian)? as usize;
        let mut off = section_start + 12 + 4 + 4;
        for _ in 0..node_count {
            let rotation = [
                read_f32(data, off, big_endian)?,
                read_f32(data, off + 4, big_endian)?,
                read_f32(data, off + 8, big_endian)?,
                read_f32(data, off + 12, big_endian)?,
            ];
            off += 16 + 16; // rotation quat, then a skipped (scale-rotation) quat
            let position = [
                read_f32(data, off, big_endian)?,
                read_f32(data, off + 4, big_endian)?,
                read_f32(data, off + 8, big_endian)?,
            ];
            off += 12 + 32; // position, then skipped fields (scale + bind-related vectors)
            let parent_raw = read_u32(data, off, big_endian)?;
            off += 4 + 76; // parent index, then skipped fields (bounding volume etc.)
            let name_len = read_u32(data, off, big_endian)? as usize;
            off += 4;
            let name = read_string(data, off, name_len)?;
            off += name_len;

            nodes.push(SkeletonNode {
                name,
                parent_index: if (parent_raw as usize) < node_count { Some(parent_raw as usize) } else { None },
                rotation,
                position,
            });
        }
    }
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_node(buf: &mut Vec<u8>, rotation: [f32; 4], position: [f32; 3], parent_index: u32, name: &str, big_endian: bool) {
        let push_f32 = |buf: &mut Vec<u8>, v: f32| buf.extend_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
        let push_u32 = |buf: &mut Vec<u8>, v: u32| buf.extend_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
        for v in rotation {
            push_f32(buf, v);
        }
        buf.extend_from_slice(&[0u8; 16]); // skipped scale-rotation quat
        for v in position {
            push_f32(buf, v);
        }
        buf.extend_from_slice(&[0u8; 32]); // skipped fields
        push_u32(buf, parent_index);
        buf.extend_from_slice(&[0u8; 76]); // skipped fields
        push_u32(buf, name.len() as u32);
        buf.extend_from_slice(name.as_bytes());
    }

    /// Builds a minimal synthetic `._xmac`-shaped buffer with just a nodes section, matching
    /// the documented real layout, so tests don't depend on committing real (licensed) game
    /// assets to the repo.
    fn synthetic_xmac_endian(nodes: &[(&str, u32, [f32; 4], [f32; 3])], big_endian: bool) -> Vec<u8> {
        let push_u32 = |buf: &mut Vec<u8>, v: u32| buf.extend_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
        let mut section_body = Vec::new();
        push_u32(&mut section_body, nodes.len() as u32);
        section_body.extend_from_slice(&[0u8; 4]);
        for (name, parent, rot, pos) in nodes {
            push_node(&mut section_body, *rot, *pos, *parent, name, big_endian);
        }

        let mut section = Vec::new();
        push_u32(&mut section, SECTION_NODES);
        push_u32(&mut section, section_body.len() as u32);
        push_u32(&mut section, 1); // version
        section.extend_from_slice(&section_body);

        let mut header = vec![0u8; 148];
        header[140..143].copy_from_slice(b"XAC");
        header[146] = if big_endian { 1 } else { 0 };
        let end_section_offset = 148 + section.len();
        // The offset-136 field is always little-endian (read before the endianness flag).
        header[136..140].copy_from_slice(&((end_section_offset - 140) as u32).to_le_bytes());

        let mut buf = header;
        buf.extend_from_slice(&section);
        buf
    }

    fn synthetic_xmac(nodes: &[(&str, u32, [f32; 4], [f32; 3])]) -> Vec<u8> {
        synthetic_xmac_endian(nodes, false)
    }

    #[test]
    fn rejects_too_short_data() {
        assert!(parse_skeleton(b"too short").is_err());
    }

    #[test]
    fn rejects_missing_xac_magic() {
        let data = vec![0u8; 200];
        assert!(parse_skeleton(&data).is_err());
    }

    #[test]
    fn parses_a_real_shaped_bone_hierarchy_with_correct_parent_links() {
        let data = synthetic_xmac(&[
            ("Bone_ROOT", u32::MAX, [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0]),
            ("Bone_Spine1", 0, [0.1, 0.2, 0.3, 0.9], [0.0, 5.0, 0.0]),
            ("Bone_Spine2", 1, [0.0, 0.0, 0.0, 1.0], [0.0, 6.0, 0.0]),
        ]);
        let nodes = parse_skeleton(&data).unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].name, "Bone_ROOT");
        assert_eq!(nodes[0].parent_index, None);
        assert_eq!(nodes[1].name, "Bone_Spine1");
        assert_eq!(nodes[1].parent_index, Some(0));
        assert_eq!(nodes[1].position, [0.0, 5.0, 0.0]);
        assert_eq!(nodes[2].parent_index, Some(1));
        assert_eq!(nodes[2].rotation, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn parses_a_real_shaped_big_endian_actor_correctly() {
        // Confirmed real: some real Risen 1 actors do set the big-endian flag (byte 146) —
        // this isn't a hypothetical, it broke a real actor selection during owner testing.
        let data = synthetic_xmac_endian(
            &[
                ("Bone_ROOT", u32::MAX, [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0]),
                ("Bone_Spine1", 0, [0.1, 0.2, 0.3, 0.9], [1.5, 5.0, -2.5]),
            ],
            true,
        );
        let nodes = parse_skeleton(&data).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].parent_index, None);
        assert_eq!(nodes[1].parent_index, Some(0));
        assert_eq!(nodes[1].position, [1.5, 5.0, -2.5]);
        assert_eq!(nodes[1].rotation, [0.1, 0.2, 0.3, 0.9]);
    }
}
