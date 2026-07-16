//! Risen 1 `._xmac` actor file — the **skinned mesh** (mesh + skin/bone-weight sections),
//! parsed directly from raw bytes in Rust rather than going through `mimicry-helper`'s OBJ
//! export. OBJ can't carry per-vertex bone weights at all, so real mesh-skinning (the surface
//! deforming with the skeleton, not just a moving stick-figure — see `xmac::parse_skeleton`
//! and `xmot::parse_motion`) needs this: a from-scratch Rust port of `ESection_Mesh` and
//! `ESection_Skin` in the vendored, working C++ reference
//! (`mimicry-helper/vendor/mimicry-source/Mimicry/mi_xmacreader.cpp`).
//!
//! Real layout quirks worth remembering if this needs revisiting:
//! - Vertex/normal/UV sub-blocks are written in **raw stream order** (`0..uniqueVertCount`), and
//!   a separate `BaseVerts` sub-block maps each raw index to its **final** position in the
//!   output vertex array (`final = arrBaseVerts[raw]`) — real games routinely have more raw
//!   entries than final vertices (or vice versa) because per-corner UV splits duplicate a
//!   shared position across several raw entries that all collapse to the same final vertex.
//! - Face indices in the raw stream are **also** in raw-index space and need the same
//!   `arrBaseVerts` remap applied after reading (done once, inside `parse_mesh_section` —
//!   faces coming back out of it are already final-space, unlike positions/normals/uvs which
//!   the caller remaps itself since it needs the raw arrays for the skin section too).
//! - The skin section's per-vertex weight-count table is indexed by **final** vertex already
//!   (the real reader passes its loop counter straight to `mCSkin::InitSwapping` with no
//!   `arrBaseVerts` remap anywhere in that code path) — unlike the mesh's own vertex/normal/UV
//!   data. Applying the remap here too was a real, found-the-hard-way bug: it silently
//!   scattered weight data onto unrelated vertices, rendering as a "torn"/mangled mesh once
//!   animated (a bone far from a given vertex ended up controlling it). See
//!   `skin_weights_index_by_final_vertex_not_raw_vertex` for the regression test.
//! - A real actor can have more than one mesh section (sub-meshes/attachments); `mimicry`'s own
//!   OBJ writer concatenates them in file order with a running vertex offset — this does the
//!   same, so vertex numbering here matches what the existing OBJ pipeline already produces.
//! - Positions/normals are a **plain passthrough** — no coordinate conversion — to agree with
//!   `xmac::parse_skeleton`'s bones, which also use plain identity. An earlier version negated
//!   Z here (verified only against `mimicry-helper`'s separately-converted `.obj`, a third,
//!   independent convention), which put the mesh and skeleton in *different* spaces — real
//!   owner testing found the mesh and skeleton facing opposite directions regardless of the
//!   `mirrored` toggle (which flips both equally, so it can't fix a mismatch between them).

use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::HashMap;

const SECTION_MESH: u32 = 1;
const SECTION_SKIN: u32 = 2;
const SECTION_MATERIALS: u32 = 13;

const MESH_SECTION_VERTICES: u32 = 0;
const MESH_SECTION_NORMALS: u32 = 1;
const MESH_SECTION_TEXCOORDS: u32 = 3;
const MESH_SECTION_BASEVERTS: u32 = 5;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkinnedMesh {
    /// Final vertex positions, one `[x, y, z]` per vertex.
    pub positions: Vec<[f32; 3]>,
    /// Parallel to `positions`; all-zero entries if the file had no normals sub-block.
    pub normals: Vec<[f32; 3]>,
    /// Parallel to `positions`; all-zero entries if the file had no UV sub-block.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle vertex indices into `positions`, 3 per face.
    pub faces: Vec<[u32; 3]>,
    /// Per vertex (parallel to `positions`): `(boneNodeIndex, weight)` pairs — indices into the
    /// same skeleton node list `xmac::parse_skeleton` returns. Empty for an unskinned vertex.
    pub skin_weights: Vec<Vec<(u32, f32)>>,
}

fn read_u32(data: &[u8], off: usize) -> Result<u32> {
    let bytes: [u8; 4] = data.get(off..off + 4).ok_or_else(|| anyhow::anyhow!("xmesh: unexpected end of file at offset {off}"))?.try_into().unwrap();
    Ok(u32::from_le_bytes(bytes))
}
fn read_u16(data: &[u8], off: usize) -> Result<u16> {
    let bytes: [u8; 2] = data.get(off..off + 2).ok_or_else(|| anyhow::anyhow!("xmesh: unexpected end of file at offset {off}"))?.try_into().unwrap();
    Ok(u16::from_le_bytes(bytes))
}
fn read_f32(data: &[u8], off: usize) -> Result<f32> {
    Ok(f32::from_bits(read_u32(data, off)?))
}

struct RawMesh {
    node_index: usize,
    /// Raw-stream-order (0..unique_vert_count); index into these with a RAW vertex index.
    raw_positions: Vec<[f32; 3]>,
    raw_normals: Vec<[f32; 3]>,
    raw_uvs: Vec<[f32; 2]>,
    /// `final_index = base_verts[raw_index]` — the same remap table used for the skin section.
    base_verts: Vec<u32>,
    final_vert_count: usize,
    /// Faces already remapped to FINAL vertex-index space (0..final_vert_count).
    faces_final: Vec<[u32; 3]>,
}

/// Parses every mesh section (each may belong to a different node) plus every skin section,
/// concatenating same as `mimicry`'s own OBJ writer does (running vertex offset, file order),
/// so vertex numbering here matches the existing `.obj` pipeline's.
pub fn parse_skinned_mesh(data: &[u8]) -> Result<SkinnedMesh> {
    if data.len() < 150 {
        bail!("xmesh: file too short to be a valid ._xmac actor ({} bytes)", data.len());
    }
    let end_section_offset = read_u32(data, 136)? as usize + 140;
    if data.get(140..143) != Some(b"XAC") {
        bail!("xmesh: missing 'XAC' magic at offset 140");
    }
    if data[146] != 0 {
        bail!("xmesh: big-endian actor files aren't supported for mesh/skin parsing yet");
    }

    let mut raw_meshes: Vec<RawMesh> = Vec::new();
    // (node_index, weights, bone_indices, per-raw-vertex weight count)
    let mut skin_entries: Vec<(usize, Vec<f32>, Vec<u16>, Vec<u32>)> = Vec::new();

    let mut next_section = 148usize;
    while next_section < end_section_offset {
        let section_id = read_u32(data, next_section)?;
        let declared_size = read_u32(data, next_section + 4)? as usize + 12;
        let section_start = next_section;

        if section_id == SECTION_MATERIALS {
            // The real reader does NOT trust this section's declared size — material entries
            // have variable-length name/texture-path strings, so it parses field-by-field and
            // uses the stream's actual position afterward as the true next-section boundary.
            // Trusting `declared_size` here (like every other section can) reads garbage for
            // everything after it — this was a real bug caught by a synthetic-vs-real mismatch.
            next_section = materials_section_end(data, section_start)?;
        } else {
            next_section += declared_size;
        }

        if section_id == SECTION_MESH {
            raw_meshes.push(parse_mesh_section(data, section_start)?);
        } else if section_id == SECTION_SKIN {
            skin_entries.push(parse_skin_section(data, section_start)?);
        }
    }

    let mut out = SkinnedMesh::default();
    let mut vert_offset = 0u32;
    // Only the running vertex offset is needed for the skin section below — its own per-vertex
    // index is already in FINAL vertex-index space (see the note above `parse_skin_section`),
    // unlike positions/normals/UVs/faces, which are raw-space and need `base_verts` themselves.
    let mut node_offsets: HashMap<usize, (u32, usize)> = HashMap::new();

    for mesh in &raw_meshes {
        let mut positions = vec![[0.0f32; 3]; mesh.final_vert_count];
        let mut normals = vec![[0.0f32; 3]; mesh.final_vert_count];
        let mut uvs = vec![[0.0f32; 2]; mesh.final_vert_count];
        for (raw_i, &final_i) in mesh.base_verts.iter().enumerate() {
            let fi = final_i as usize;
            // Deliberately a plain passthrough, NOT the Z-negation an earlier version of this
            // had (verified back then only against `mimicry-helper`'s separately-converted
            // `.obj` — a third, independent coordinate convention this code no longer needs to
            // match). What actually matters is agreeing with `xmac::parse_skeleton`'s bones,
            // which use plain identity — real owner testing found the mesh and skeleton facing
            // opposite directions *regardless* of the `mirrored` toggle (which flips both
            // equally, so it can't fix a mismatch between them), proving they need to share
            // one convention here, not each separately match the `.obj`.
            if let Some(p) = mesh.raw_positions.get(raw_i) {
                positions[fi] = *p;
            }
            if let Some(n) = mesh.raw_normals.get(raw_i) {
                normals[fi] = *n;
            }
            if let Some(uv) = mesh.raw_uvs.get(raw_i) {
                uvs[fi] = *uv;
            }
        }
        out.positions.extend(positions);
        out.normals.extend(normals);
        out.uvs.extend(uvs);
        for face in &mesh.faces_final {
            // No winding reversal either now — that was only compensating for the Z-negation
            // above, which is gone.
            out.faces.push([face[0] + vert_offset, face[1] + vert_offset, face[2] + vert_offset]);
        }
        node_offsets.insert(mesh.node_index, (vert_offset, mesh.final_vert_count));
        vert_offset += mesh.final_vert_count as u32;
    }
    out.skin_weights = vec![Vec::new(); vert_offset as usize];

    for (node_index, weights, bone_indices, per_vert_count) in &skin_entries {
        let Some(&(offset, final_vert_count)) = node_offsets.get(node_index) else { continue };
        let mut w_i = 0usize;
        // The skin section's own per-vertex loop counter is already a FINAL vertex index (the
        // real reader passes it straight to `mCSkin::InitSwapping` with no `base_verts` remap
        // anywhere in that code path) — unlike the mesh's raw vertex/normal/UV data, which does
        // need that remap. Applying it here too (an earlier real bug) silently scattered weight
        // data onto unrelated vertices — it rendered as a "torn"/mangled mesh once animated,
        // since a bone far from a given vertex would end up controlling it.
        for (final_vert, &count) in per_vert_count.iter().enumerate() {
            if final_vert >= final_vert_count {
                w_i += count as usize;
                continue;
            }
            let out_i = offset as usize + final_vert;
            for _ in 0..count {
                if w_i < weights.len() {
                    out.skin_weights[out_i].push((bone_indices[w_i] as u32, weights[w_i]));
                }
                w_i += 1;
            }
        }
    }

    Ok(out)
}

/// Walks a materials section field-by-field (matching the real reader exactly) purely to find
/// where it really ends — not needed for skinning itself, but every section after this one in
/// the file is unreadable garbage without it (this section's own declared size can't be trusted).
fn materials_section_end(data: &[u8], section_start: usize) -> Result<usize> {
    let mut off = section_start + 12; // past {id, size, version}
    let material_count = read_u32(data, off)?;
    off += 4;
    off += 8; // skip
    for _ in 0..material_count {
        off += 95; // skip
        let map_count = *data.get(off).ok_or_else(|| anyhow::anyhow!("xmesh: unexpected end of file at offset {off}"))?;
        off += 1;
        let name_len = read_u32(data, off)? as usize;
        off += 4 + name_len;
        for _ in 0..map_count {
            off += 26; // skip
            off += 1; // map type byte
            off += 1; // skip
            let path_len = read_u32(data, off)? as usize;
            off += 4 + path_len;
        }
    }
    Ok(off)
}

fn parse_mesh_section(data: &[u8], section_start: usize) -> Result<RawMesh> {
    let mut off = section_start + 12; // past {id,size,version}
    let node_index = read_u32(data, off)? as usize;
    off += 4;
    let vert_count = read_u32(data, off)? as usize;
    off += 4;
    let uvert_count = read_u32(data, off)? as usize;
    off += 4;
    let face_count = read_u32(data, off)? as usize / 3;
    off += 4;
    off += 4; // skip
    let mesh_section_count = read_u32(data, off)?;
    off += 4;
    off += 4; // skip

    let mut base_verts: Vec<u32> = (0..uvert_count as u32).collect();
    let mut raw_positions = Vec::new();
    let mut raw_normals = Vec::new();
    let mut raw_uvs = Vec::new();

    for _ in 0..mesh_section_count {
        let sub_id = read_u32(data, off)?;
        let block_size = read_u32(data, off + 4)?;
        off += 12; // id, size, then a skipped field
        if sub_id == MESH_SECTION_VERTICES && block_size == 12 {
            raw_positions = (0..uvert_count)
                .map(|v| {
                    let o = off + v * 12;
                    Ok([read_f32(data, o)?, read_f32(data, o + 4)?, read_f32(data, o + 8)?])
                })
                .collect::<Result<Vec<_>>>()?;
            off += uvert_count * 12;
        } else if sub_id == MESH_SECTION_NORMALS && block_size == 12 {
            raw_normals = (0..uvert_count)
                .map(|v| {
                    let o = off + v * 12;
                    Ok([read_f32(data, o)?, read_f32(data, o + 4)?, read_f32(data, o + 8)?])
                })
                .collect::<Result<Vec<_>>>()?;
            off += uvert_count * 12;
        } else if sub_id == MESH_SECTION_TEXCOORDS && block_size == 8 {
            raw_uvs = (0..uvert_count)
                .map(|v| {
                    let o = off + v * 8;
                    Ok([read_f32(data, o)?, read_f32(data, o + 4)?])
                })
                .collect::<Result<Vec<_>>>()?;
            off += uvert_count * 8;
        } else if sub_id == MESH_SECTION_BASEVERTS && block_size == 4 {
            base_verts = (0..uvert_count).map(|v| read_u32(data, off + v * 4)).collect::<Result<Vec<_>>>()?;
            off += uvert_count * 4;
        } else {
            // Tangents (id 2) or any other sub-block this doesn't need: skip its raw bytes.
            off += (block_size as usize) * uvert_count;
        }
    }

    // Face parts: each part is {faceCountX3, vertCount, materialId, skipCount} then that many
    // faces (3 raw u32 indices each, offset by the running per-part vertex count).
    let mut faces_raw = Vec::with_capacity(face_count);
    let mut passed_faces = 0usize;
    let mut passed_verts = 0u32;
    while passed_faces != face_count {
        let part_face_count = read_u32(data, off)? as usize / 3;
        off += 4;
        let part_vert_count = read_u32(data, off)?;
        off += 4;
        off += 4; // material id (not used here — single-texture rendering for now)
        let skip_words = read_u32(data, off)?;
        off += 4;
        for _ in 0..part_face_count {
            let a = read_u32(data, off)? + passed_verts;
            let b = read_u32(data, off + 4)? + passed_verts;
            let c = read_u32(data, off + 8)? + passed_verts;
            faces_raw.push([a, b, c]);
            off += 12;
        }
        off += (skip_words as usize) * 4;
        passed_faces += part_face_count;
        passed_verts += part_vert_count;
    }
    let faces_final: Vec<[u32; 3]> = faces_raw
        .iter()
        .map(|f| [base_verts[f[0] as usize], base_verts[f[1] as usize], base_verts[f[2] as usize]])
        .collect();

    Ok(RawMesh {
        node_index,
        raw_positions,
        raw_normals,
        raw_uvs,
        base_verts,
        final_vert_count: vert_count,
        faces_final,
    })
}

fn parse_skin_section(data: &[u8], section_start: usize) -> Result<(usize, Vec<f32>, Vec<u16>, Vec<u32>)> {
    let mut off = section_start + 12;
    let node_index = read_u32(data, off)? as usize;
    off += 4;
    off += 4; // skip
    let weight_count = read_u32(data, off)? as usize;
    off += 4;
    off += 4; // skip

    let mut weights = Vec::with_capacity(weight_count);
    let mut bone_indices = Vec::with_capacity(weight_count);
    for _ in 0..weight_count {
        weights.push(read_f32(data, off)?);
        off += 4;
        bone_indices.push(read_u16(data, off)?);
        off += 2;
        off += 2; // skip
    }

    let mut per_raw_vert_count = Vec::new();
    let mut consumed = 0usize;
    while consumed < weight_count {
        off += 4; // unused u32
        let count = read_u32(data, off)?;
        off += 4;
        per_raw_vert_count.push(count);
        consumed += count as usize;
    }

    Ok((node_index, weights, bone_indices, per_raw_vert_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_section(buf: &mut Vec<u8>, id: u32, body: &[u8]) {
        buf.extend_from_slice(&id.to_le_bytes());
        buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes()); // version
        buf.extend_from_slice(body);
    }

    /// One triangle (3 raw verts, no UV-split duplicates), skinned: v0 fully on bone 5, v1
    /// split 50/50 between bones 3 and 7, v2 unskinned (not listed in the skin section at all).
    fn synthetic_xmac_with_mesh_and_skin() -> Vec<u8> {
        let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let mut mesh_body = Vec::new();
        mesh_body.extend_from_slice(&0u32.to_le_bytes()); // node_index
        mesh_body.extend_from_slice(&3u32.to_le_bytes()); // vert_count (final)
        mesh_body.extend_from_slice(&3u32.to_le_bytes()); // uvert_count (raw)
        mesh_body.extend_from_slice(&(1u32 * 3).to_le_bytes()); // face_count * 3
        mesh_body.extend_from_slice(&[0u8; 4]);
        mesh_body.extend_from_slice(&1u32.to_le_bytes()); // mesh_section_count (vertices only)
        mesh_body.extend_from_slice(&[0u8; 4]);
        // Vertices sub-block (id=0, blockSize=12)
        mesh_body.extend_from_slice(&MESH_SECTION_VERTICES.to_le_bytes());
        mesh_body.extend_from_slice(&12u32.to_le_bytes());
        mesh_body.extend_from_slice(&[0u8; 4]);
        for p in positions {
            for c in p {
                mesh_body.extend_from_slice(&c.to_le_bytes());
            }
        }
        // one face part: {faceCountX3, vertCount, materialId, skipWords} + indices
        mesh_body.extend_from_slice(&3u32.to_le_bytes());
        mesh_body.extend_from_slice(&3u32.to_le_bytes());
        mesh_body.extend_from_slice(&0u32.to_le_bytes());
        mesh_body.extend_from_slice(&0u32.to_le_bytes());
        for i in [0u32, 1, 2] {
            mesh_body.extend_from_slice(&i.to_le_bytes());
        }

        let mut skin_body = Vec::new();
        skin_body.extend_from_slice(&0u32.to_le_bytes()); // node_index (same as mesh)
        skin_body.extend_from_slice(&[0u8; 4]);
        skin_body.extend_from_slice(&3u32.to_le_bytes()); // weight_count (1 + 2 + 0)
        skin_body.extend_from_slice(&[0u8; 4]);
        for (weight, bone) in [(1.0f32, 5u16), (0.5, 3), (0.5, 7)] {
            skin_body.extend_from_slice(&weight.to_le_bytes());
            skin_body.extend_from_slice(&bone.to_le_bytes());
            skin_body.extend_from_slice(&[0u8; 2]);
        }
        for count in [1u32, 2, 0] {
            skin_body.extend_from_slice(&[0u8; 4]);
            skin_body.extend_from_slice(&count.to_le_bytes());
        }

        let mut sections = Vec::new();
        push_section(&mut sections, SECTION_MESH, &mesh_body);
        push_section(&mut sections, SECTION_SKIN, &skin_body);

        let mut header = vec![0u8; 148];
        header[140..143].copy_from_slice(b"XAC");
        header[146] = 0;
        let end_section_offset = 148 + sections.len();
        header[136..140].copy_from_slice(&((end_section_offset - 140) as u32).to_le_bytes());

        let mut buf = header;
        buf.extend_from_slice(&sections);
        buf
    }

    /// A mesh where 4 raw (pre-UV-split) vertices collapse to 2 final vertices via `BaseVerts`
    /// (raw 0,1 -> final 0; raw 2,3 -> final 1) — the exact shape that exposed the real bug:
    /// skin weights are indexed by FINAL vertex directly (2 entries here), not by raw vertex,
    /// and applying the raw->final remap to them a second time scattered weights onto the
    /// wrong vertices (this rendered as a "torn"/mangled mesh once animated in the real app).
    fn synthetic_xmac_with_collapsed_verts() -> Vec<u8> {
        let raw_positions: [[f32; 3]; 4] = [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let base_verts: [u32; 4] = [0, 0, 1, 1];
        let mut mesh_body = Vec::new();
        mesh_body.extend_from_slice(&0u32.to_le_bytes()); // node_index
        mesh_body.extend_from_slice(&2u32.to_le_bytes()); // vert_count (final)
        mesh_body.extend_from_slice(&4u32.to_le_bytes()); // uvert_count (raw)
        mesh_body.extend_from_slice(&0u32.to_le_bytes()); // face_count * 3 (no faces needed here)
        mesh_body.extend_from_slice(&[0u8; 4]);
        mesh_body.extend_from_slice(&2u32.to_le_bytes()); // mesh_section_count (vertices + base verts)
        mesh_body.extend_from_slice(&[0u8; 4]);
        mesh_body.extend_from_slice(&MESH_SECTION_VERTICES.to_le_bytes());
        mesh_body.extend_from_slice(&12u32.to_le_bytes());
        mesh_body.extend_from_slice(&[0u8; 4]);
        for p in raw_positions {
            for c in p {
                mesh_body.extend_from_slice(&c.to_le_bytes());
            }
        }
        mesh_body.extend_from_slice(&MESH_SECTION_BASEVERTS.to_le_bytes());
        mesh_body.extend_from_slice(&4u32.to_le_bytes());
        mesh_body.extend_from_slice(&[0u8; 4]);
        for v in base_verts {
            mesh_body.extend_from_slice(&v.to_le_bytes());
        }
        // no face parts (passed_faces starts at 0 == face_count, loop never runs)

        let mut skin_body = Vec::new();
        skin_body.extend_from_slice(&0u32.to_le_bytes()); // node_index
        skin_body.extend_from_slice(&[0u8; 4]);
        skin_body.extend_from_slice(&2u32.to_le_bytes()); // weight_count
        skin_body.extend_from_slice(&[0u8; 4]);
        for (weight, bone) in [(1.0f32, 9u16), (1.0, 11)] {
            skin_body.extend_from_slice(&weight.to_le_bytes());
            skin_body.extend_from_slice(&bone.to_le_bytes());
            skin_body.extend_from_slice(&[0u8; 2]);
        }
        for count in [1u32, 1] {
            // 2 entries: one per FINAL vertex, not per raw vertex
            skin_body.extend_from_slice(&[0u8; 4]);
            skin_body.extend_from_slice(&count.to_le_bytes());
        }

        let mut sections = Vec::new();
        push_section(&mut sections, SECTION_MESH, &mesh_body);
        push_section(&mut sections, SECTION_SKIN, &skin_body);

        let mut header = vec![0u8; 148];
        header[140..143].copy_from_slice(b"XAC");
        header[146] = 0;
        let end_section_offset = 148 + sections.len();
        header[136..140].copy_from_slice(&((end_section_offset - 140) as u32).to_le_bytes());

        let mut buf = header;
        buf.extend_from_slice(&sections);
        buf
    }

    #[test]
    fn skin_weights_index_by_final_vertex_not_raw_vertex() {
        let data = synthetic_xmac_with_collapsed_verts();
        let mesh = parse_skinned_mesh(&data).unwrap();
        assert_eq!(mesh.positions.len(), 2);
        assert_eq!(mesh.skin_weights.len(), 2);
        assert_eq!(mesh.skin_weights[0], vec![(9, 1.0)]);
        assert_eq!(mesh.skin_weights[1], vec![(11, 1.0)]);
    }

    #[test]
    fn parses_real_shaped_mesh_and_skin_as_plain_passthrough() {
        let data = synthetic_xmac_with_mesh_and_skin();
        let mesh = parse_skinned_mesh(&data).unwrap();
        assert_eq!(mesh.positions.len(), 3);
        // Plain passthrough — no coordinate conversion here, to match
        // `xmac::parse_skeleton`'s bones exactly (see the module doc for why).
        assert_eq!(mesh.positions[0], [0.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[2], [0.0, 1.0, 0.0]);
        assert_eq!(mesh.faces, vec![[0, 1, 2]]);
        assert_eq!(mesh.skin_weights[0], vec![(5, 1.0)]);
        assert_eq!(mesh.skin_weights[1], vec![(3, 0.5), (7, 0.5)]);
        assert_eq!(mesh.skin_weights[2], Vec::new());
    }

    #[test]
    fn rejects_too_short_data() {
        assert!(parse_skinned_mesh(b"too short").is_err());
    }
}
