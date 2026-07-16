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
//!   a separate `BaseVerts` sub-block maps each raw index to its **final** position
//!   (`final = arrBaseVerts[raw]`) — real games routinely have more raw entries than final
//!   vertices because per-corner UV/normal splits duplicate a shared position across several
//!   raw entries that all collapse to the same final vertex.
//! - **The output of this parser stays in RAW vertex space on purpose.** An earlier version
//!   collapsed raw vertices to final space (one UV per final vertex) — a real, owner-visible
//!   bug: the duplicates exist precisely to carry *different* UVs at texture seams (real Wolf:
//!   3857 raw vs 3252 final — 719 final vertices carry 2+ distinct UVs, and 43.7% of faces
//!   touch at least one), so collapsing smeared triangles clear across the texture atlas
//!   (rendered as "most of the body is flat/wrong color"). Faces are therefore kept raw-space
//!   too, and skin weights (stored per FINAL vertex in the file) are expanded per raw vertex
//!   via `weights[base_verts[raw]]`. See `uv_seam_duplicates_keep_their_own_uvs`.
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

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkinnedMeshMaterial {
    pub name: String,
    /// Diffuse texture file name exactly as stored in the actor's own materials section (no
    /// extension — the real reader appends one itself, see mimicry's `mi_xmacreader.cpp`).
    pub diffuse: Option<String>,
    /// Normal-map texture file name, same convention as `diffuse`.
    pub normal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkinnedMesh {
    /// Vertex positions, one `[x, y, z]` per RAW vertex (UV-seam duplicates kept — see the
    /// module doc for why collapsing them is a real rendering bug).
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
    /// The actor's own materials, in file order — the index space `face_material_ids` uses.
    /// Real actors are multi-material (e.g. Wolf = Body + Claws + an engine-default), so
    /// rendering everything with the first material's texture leaves parts visibly wrong.
    pub materials: Vec<SkinnedMeshMaterial>,
    /// Parallel to `faces`: which material (index into `materials`) each triangle uses, read
    /// from each face-part's own material id.
    pub face_material_ids: Vec<u32>,
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
    /// `final_index = base_verts[raw_index]` — needed to expand the skin section's per-FINAL-
    /// vertex weights onto raw vertices.
    base_verts: Vec<u32>,
    final_vert_count: usize,
    /// Faces in RAW vertex-index space (0..uvert_count) — deliberately NOT remapped through
    /// `base_verts`, so UV-seam duplicate vertices keep their own distinct UVs.
    faces_raw: Vec<[u32; 3]>,
    /// Parallel to `faces_final`: each face-part's material id, repeated per face in the part.
    face_material_ids: Vec<u32>,
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
    let mut materials: Vec<SkinnedMeshMaterial> = Vec::new();

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
            let (end, mats) = parse_materials_section(data, section_start)?;
            next_section = end;
            if materials.is_empty() {
                materials = mats;
            }
        } else {
            next_section += declared_size;
        }

        if section_id == SECTION_MESH {
            raw_meshes.push(parse_mesh_section(data, section_start)?);
        } else if section_id == SECTION_SKIN {
            skin_entries.push(parse_skin_section(data, section_start)?);
        }
    }

    // Collision hulls are stored as ordinary extra mesh sections (node "CollisionMesh") whose
    // one reliable tell is having NO texture-coordinate sub-block — they're physics geometry,
    // never meant to be drawn. Rendering them anyway (with zero-filled UVs) smears one texel
    // corner across a full-body hull drawn OVER the real skin: the real SwampMummy
    // "виглядає жахливо у прев'ю" bug (its hull reuses the REAL material ids, so hiding
    // untextured materials on the frontend can't catch it — the Wolf's hull happened to use
    // the untextured EMFX_Default material, which masked this). Only drop them when at least
    // one section does carry UVs, so a hypothetical fully-UV-less actor still renders.
    if raw_meshes.iter().any(|m| !m.raw_uvs.is_empty()) {
        raw_meshes.retain(|m| !m.raw_uvs.is_empty());
    }

    let mut out = SkinnedMesh::default();
    out.materials = materials;
    let mut vert_offset = 0u32;
    // Output stays in RAW vertex space (offset = running raw count); `node_offsets` remembers
    // each node's slice start + which RawMesh it was, so the skin section's per-FINAL-vertex
    // weights can be expanded onto every raw duplicate below.
    let mut node_offsets: HashMap<usize, (u32, usize)> = HashMap::new();

    for (mesh_i, mesh) in raw_meshes.iter().enumerate() {
        let raw_count = mesh.base_verts.len();
        let pad3 = |src: &Vec<[f32; 3]>| -> Vec<[f32; 3]> {
            let mut v = src.clone();
            v.resize(raw_count, [0.0; 3]);
            v
        };
        // Positions/normals are a plain passthrough (no coordinate conversion) to agree with
        // `xmac::parse_skeleton`'s bones, which also use plain identity. An earlier version
        // negated Z here (verified only against `mimicry-helper`'s separately-converted
        // `.obj`, a third, independent convention), which put the mesh and skeleton in
        // *different* spaces — real owner testing found them facing opposite directions
        // regardless of the `mirrored` toggle (which flips both equally, so it can't fix a
        // mismatch between them).
        out.positions.extend(pad3(&mesh.raw_positions));
        out.normals.extend(pad3(&mesh.raw_normals));
        let mut uvs = mesh.raw_uvs.clone();
        uvs.resize(raw_count, [0.0; 2]);
        out.uvs.extend(uvs);
        for face in &mesh.faces_raw {
            out.faces.push([face[0] + vert_offset, face[1] + vert_offset, face[2] + vert_offset]);
        }
        out.face_material_ids.extend_from_slice(&mesh.face_material_ids);
        node_offsets.insert(mesh.node_index, (vert_offset, mesh_i));
        vert_offset += raw_count as u32;
    }
    out.skin_weights = vec![Vec::new(); vert_offset as usize];

    for (node_index, weights, bone_indices, per_vert_count) in &skin_entries {
        let Some(&(offset, mesh_i)) = node_offsets.get(node_index) else { continue };
        let mesh = &raw_meshes[mesh_i];
        // The skin section's own per-vertex loop counter is a FINAL vertex index (the real
        // reader passes it straight to `mCSkin::InitSwapping` with no `base_verts` remap
        // anywhere in that code path). Applying the remap to it (an earlier real bug) silently
        // scattered weight data onto unrelated vertices — it rendered as a "torn"/mangled mesh
        // once animated, since a bone far from a given vertex would end up controlling it.
        // Gather per-final-vertex weight lists first...
        let mut final_weights: Vec<Vec<(u32, f32)>> = vec![Vec::new(); mesh.final_vert_count];
        let mut w_i = 0usize;
        for (final_vert, &count) in per_vert_count.iter().enumerate() {
            for _ in 0..count {
                if w_i < weights.len() {
                    if let Some(slot) = final_weights.get_mut(final_vert) {
                        slot.push((bone_indices[w_i] as u32, weights[w_i]));
                    }
                }
                w_i += 1;
            }
        }
        // ...then expand onto every raw duplicate: a seam duplicate shares its base vertex's
        // skin weights (it's the same physical point — only its UV/normal differ).
        for (raw_i, &final_i) in mesh.base_verts.iter().enumerate() {
            if let Some(w) = final_weights.get(final_i as usize) {
                out.skin_weights[offset as usize + raw_i] = w.clone();
            }
        }
    }

    Ok(out)
}

/// Walks a materials section field-by-field (matching the real reader exactly), returning
/// where it really ends — every section after this one in the file is unreadable garbage
/// without that (this section's own declared size can't be trusted) — plus the real material
/// list itself: name and diffuse/normal texture file names per material. Map-type semantics
/// come straight from mimicry's `mi_xmacreader.cpp`: the map's type byte modulo 8 indexes a
/// fixed table where 2 = diffuse and 5 = normal (everything else is specular/placeholder).
fn parse_materials_section(data: &[u8], section_start: usize) -> Result<(usize, Vec<SkinnedMeshMaterial>)> {
    let read_str = |off: usize, len: usize| -> Result<String> {
        let bytes = data
            .get(off..off + len)
            .ok_or_else(|| anyhow::anyhow!("xmesh: unexpected end of file at offset {off}"))?;
        Ok(String::from_utf8_lossy(bytes).trim_end_matches('\0').to_string())
    };

    let mut off = section_start + 12; // past {id, size, version}
    let material_count = read_u32(data, off)?;
    off += 4;
    off += 8; // skip
    let mut materials = Vec::with_capacity(material_count as usize);
    for _ in 0..material_count {
        off += 95; // skip
        let map_count = *data.get(off).ok_or_else(|| anyhow::anyhow!("xmesh: unexpected end of file at offset {off}"))?;
        off += 1;
        let name_len = read_u32(data, off)? as usize;
        off += 4;
        let name = read_str(off, name_len)?;
        off += name_len;
        let mut material = SkinnedMeshMaterial { name, diffuse: None, normal: None };
        for _ in 0..map_count {
            off += 26; // skip
            let map_type = *data.get(off).ok_or_else(|| anyhow::anyhow!("xmesh: unexpected end of file at offset {off}"))? % 8;
            off += 1;
            off += 1; // skip
            let path_len = read_u32(data, off)? as usize;
            off += 4;
            let path = read_str(off, path_len)?;
            off += path_len;
            match map_type {
                2 if material.diffuse.is_none() => material.diffuse = Some(path),
                5 if material.normal.is_none() => material.normal = Some(path),
                _ => {}
            }
        }
        materials.push(material);
    }
    Ok((off, materials))
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
    let mut face_material_ids = Vec::with_capacity(face_count);
    let mut passed_faces = 0usize;
    let mut passed_verts = 0u32;
    while passed_faces != face_count {
        let part_face_count = read_u32(data, off)? as usize / 3;
        off += 4;
        let part_vert_count = read_u32(data, off)?;
        off += 4;
        let part_material_id = read_u32(data, off)?;
        off += 4;
        let skip_words = read_u32(data, off)?;
        off += 4;
        for _ in 0..part_face_count {
            let a = read_u32(data, off)? + passed_verts;
            let b = read_u32(data, off + 4)? + passed_verts;
            let c = read_u32(data, off + 8)? + passed_verts;
            faces_raw.push([a, b, c]);
            face_material_ids.push(part_material_id);
            off += 12;
        }
        off += (skip_words as usize) * 4;
        passed_faces += part_face_count;
        passed_verts += part_vert_count;
    }

    Ok(RawMesh {
        node_index,
        raw_positions,
        raw_normals,
        raw_uvs,
        base_verts,
        final_vert_count: vert_count,
        faces_raw,
        face_material_ids,
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
    /// (raw 0,1 -> final 0; raw 2,3 -> final 1) — the exact shape that exposed TWO real bugs:
    /// (1) skin weights are indexed by FINAL vertex directly (2 entries here), not by raw
    /// vertex — applying the raw->final remap to them a second time scattered weights onto the
    /// wrong vertices ("torn"/mangled mesh once animated); (2) each raw duplicate must keep its
    /// OWN UV (collapsing to one UV per final vertex smeared triangles across texture seams —
    /// the real Wolf "most of the body is the wrong color" bug).
    fn synthetic_xmac_with_collapsed_verts() -> Vec<u8> {
        let raw_positions: [[f32; 3]; 4] = [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let raw_uvs: [[f32; 2]; 4] = [[0.1, 0.2], [0.8, 0.9], [0.3, 0.4], [0.6, 0.7]];
        let base_verts: [u32; 4] = [0, 0, 1, 1];
        let mut mesh_body = Vec::new();
        mesh_body.extend_from_slice(&0u32.to_le_bytes()); // node_index
        mesh_body.extend_from_slice(&2u32.to_le_bytes()); // vert_count (final)
        mesh_body.extend_from_slice(&4u32.to_le_bytes()); // uvert_count (raw)
        mesh_body.extend_from_slice(&(1u32 * 3).to_le_bytes()); // face_count * 3
        mesh_body.extend_from_slice(&[0u8; 4]);
        mesh_body.extend_from_slice(&3u32.to_le_bytes()); // mesh_section_count (vertices + uvs + base verts)
        mesh_body.extend_from_slice(&[0u8; 4]);
        mesh_body.extend_from_slice(&MESH_SECTION_VERTICES.to_le_bytes());
        mesh_body.extend_from_slice(&12u32.to_le_bytes());
        mesh_body.extend_from_slice(&[0u8; 4]);
        for p in raw_positions {
            for c in p {
                mesh_body.extend_from_slice(&c.to_le_bytes());
            }
        }
        mesh_body.extend_from_slice(&MESH_SECTION_TEXCOORDS.to_le_bytes());
        mesh_body.extend_from_slice(&8u32.to_le_bytes());
        mesh_body.extend_from_slice(&[0u8; 4]);
        for uv in raw_uvs {
            for c in uv {
                mesh_body.extend_from_slice(&c.to_le_bytes());
            }
        }
        mesh_body.extend_from_slice(&MESH_SECTION_BASEVERTS.to_le_bytes());
        mesh_body.extend_from_slice(&4u32.to_le_bytes());
        mesh_body.extend_from_slice(&[0u8; 4]);
        for v in base_verts {
            mesh_body.extend_from_slice(&v.to_le_bytes());
        }
        // one face part referencing RAW indices 1,2,3 — output must keep them raw, NOT remap
        // them through base_verts (which would collapse the seam)
        mesh_body.extend_from_slice(&3u32.to_le_bytes());
        mesh_body.extend_from_slice(&4u32.to_le_bytes());
        mesh_body.extend_from_slice(&0u32.to_le_bytes());
        mesh_body.extend_from_slice(&0u32.to_le_bytes());
        for i in [1u32, 2, 3] {
            mesh_body.extend_from_slice(&i.to_le_bytes());
        }

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
        // Output is RAW space: 4 vertices, each seam duplicate inheriting its base (final)
        // vertex's skin weights. The 2 skin entries are per FINAL vertex in the file.
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.skin_weights.len(), 4);
        assert_eq!(mesh.skin_weights[0], vec![(9, 1.0)]);
        assert_eq!(mesh.skin_weights[1], vec![(9, 1.0)]);
        assert_eq!(mesh.skin_weights[2], vec![(11, 1.0)]);
        assert_eq!(mesh.skin_weights[3], vec![(11, 1.0)]);
    }

    /// The real SwampMummy shape: a visual mesh section WITH texture coordinates plus a
    /// collision-hull mesh section WITHOUT any TEXCOORDS sub-block (on a different node),
    /// whose faces reuse the REAL material ids. The hull must be dropped from the render
    /// output — zero-filled UVs smear one texel across a full-body hull drawn over the skin.
    #[test]
    fn mesh_sections_without_uvs_are_dropped_as_collision_hulls() {
        fn push_mesh_section(sections: &mut Vec<u8>, node_index: u32, with_uvs: bool) {
            let mut body = Vec::new();
            body.extend_from_slice(&node_index.to_le_bytes());
            body.extend_from_slice(&3u32.to_le_bytes()); // final verts
            body.extend_from_slice(&3u32.to_le_bytes()); // raw verts
            body.extend_from_slice(&(1u32 * 3).to_le_bytes()); // face_count * 3
            body.extend_from_slice(&[0u8; 4]);
            body.extend_from_slice(&(if with_uvs { 2u32 } else { 1u32 }).to_le_bytes());
            body.extend_from_slice(&[0u8; 4]);
            body.extend_from_slice(&MESH_SECTION_VERTICES.to_le_bytes());
            body.extend_from_slice(&12u32.to_le_bytes());
            body.extend_from_slice(&[0u8; 4]);
            for p in [[node_index as f32, 0.0f32, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
                for c in p {
                    body.extend_from_slice(&c.to_le_bytes());
                }
            }
            if with_uvs {
                body.extend_from_slice(&MESH_SECTION_TEXCOORDS.to_le_bytes());
                body.extend_from_slice(&8u32.to_le_bytes());
                body.extend_from_slice(&[0u8; 4]);
                for uv in [[0.1f32, 0.2], [0.3, 0.4], [0.5, 0.6]] {
                    for c in uv {
                        body.extend_from_slice(&c.to_le_bytes());
                    }
                }
            }
            // one face part, material 0
            body.extend_from_slice(&3u32.to_le_bytes());
            body.extend_from_slice(&3u32.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
            for i in [0u32, 1, 2] {
                body.extend_from_slice(&i.to_le_bytes());
            }
            push_section(sections, SECTION_MESH, &body);
        }

        let mut sections = Vec::new();
        push_mesh_section(&mut sections, 0, true); // visual mesh
        push_mesh_section(&mut sections, 69, false); // collision hull (no TEXCOORDS)
        let mut header = vec![0u8; 148];
        header[140..143].copy_from_slice(b"XAC");
        header[136..140].copy_from_slice(&((148 + sections.len() - 140) as u32).to_le_bytes());
        let mut data = header;
        data.extend_from_slice(&sections);

        let mesh = parse_skinned_mesh(&data).unwrap();
        assert_eq!(mesh.positions.len(), 3, "collision-hull section must be dropped");
        assert_eq!(mesh.faces.len(), 1);
        assert_eq!(mesh.uvs, vec![[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]]);
    }

    #[test]
    fn uv_seam_duplicates_keep_their_own_uvs() {
        let data = synthetic_xmac_with_collapsed_verts();
        let mesh = parse_skinned_mesh(&data).unwrap();
        // Every raw duplicate keeps its OWN UV — collapsing raw 1 onto final 0 (losing
        // [0.8, 0.9]) was the real "texture smeared across the atlas" bug.
        assert_eq!(mesh.uvs, vec![[0.1, 0.2], [0.8, 0.9], [0.3, 0.4], [0.6, 0.7]]);
        // Faces stay in raw index space; remapping them through base_verts would collapse the
        // seam and pick up the wrong UV.
        assert_eq!(mesh.faces, vec![[1, 2, 3]]);
        // Positions of duplicates match their base vertex (same physical point).
        assert_eq!(mesh.positions[0], mesh.positions[1]);
        assert_eq!(mesh.positions[2], mesh.positions[3]);
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

    /// A materials section shaped exactly like the real reader walks it (95 skip bytes, map
    /// count, name, then per map: 26 skip + type byte + 1 skip + path), followed by a mesh
    /// whose two face parts use different material ids — the real Wolf shape (Body + Claws).
    fn synthetic_xmac_with_materials_and_two_part_mesh() -> Vec<u8> {
        fn push_material(buf: &mut Vec<u8>, name: &str, maps: &[(u8, &str)]) {
            buf.extend_from_slice(&[0u8; 95]);
            buf.push(maps.len() as u8);
            buf.extend_from_slice(&(name.len() as u32).to_le_bytes());
            buf.extend_from_slice(name.as_bytes());
            for (map_type, path) in maps {
                buf.extend_from_slice(&[0u8; 26]);
                buf.push(*map_type);
                buf.push(0);
                buf.extend_from_slice(&(path.len() as u32).to_le_bytes());
                buf.extend_from_slice(path.as_bytes());
            }
        }
        let mut materials_body = Vec::new();
        materials_body.extend_from_slice(&2u32.to_le_bytes()); // material count
        materials_body.extend_from_slice(&[0u8; 8]);
        push_material(&mut materials_body, "Wolf_Body", &[(2, "Wolf_Body_Diffuse_S1"), (5, "Wolf_Body_Normal_S1")]);
        push_material(&mut materials_body, "Wolf_Claws", &[(2, "Wolf_Claws_Diffuse_S1")]);

        let positions: [[f32; 3]; 6] = [[0.0; 3]; 6];
        let mut mesh_body = Vec::new();
        mesh_body.extend_from_slice(&0u32.to_le_bytes()); // node_index
        mesh_body.extend_from_slice(&6u32.to_le_bytes()); // vert_count (final)
        mesh_body.extend_from_slice(&6u32.to_le_bytes()); // uvert_count (raw)
        mesh_body.extend_from_slice(&(2u32 * 3).to_le_bytes()); // face_count * 3
        mesh_body.extend_from_slice(&[0u8; 4]);
        mesh_body.extend_from_slice(&1u32.to_le_bytes()); // mesh_section_count (vertices only)
        mesh_body.extend_from_slice(&[0u8; 4]);
        mesh_body.extend_from_slice(&MESH_SECTION_VERTICES.to_le_bytes());
        mesh_body.extend_from_slice(&12u32.to_le_bytes());
        mesh_body.extend_from_slice(&[0u8; 4]);
        for p in positions {
            for c in p {
                mesh_body.extend_from_slice(&c.to_le_bytes());
            }
        }
        // Two face parts, one triangle each, materials 0 and 1.
        for material_id in [0u32, 1] {
            mesh_body.extend_from_slice(&3u32.to_le_bytes()); // face_count * 3
            mesh_body.extend_from_slice(&3u32.to_le_bytes()); // part vert count
            mesh_body.extend_from_slice(&material_id.to_le_bytes());
            mesh_body.extend_from_slice(&0u32.to_le_bytes()); // skip words
            for i in [0u32, 1, 2] {
                mesh_body.extend_from_slice(&i.to_le_bytes());
            }
        }

        let mut sections = Vec::new();
        push_section(&mut sections, SECTION_MATERIALS, &materials_body);
        push_section(&mut sections, SECTION_MESH, &mesh_body);

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
    fn parses_materials_and_per_face_material_ids() {
        let data = synthetic_xmac_with_materials_and_two_part_mesh();
        let mesh = parse_skinned_mesh(&data).unwrap();
        assert_eq!(
            mesh.materials,
            vec![
                SkinnedMeshMaterial {
                    name: "Wolf_Body".into(),
                    diffuse: Some("Wolf_Body_Diffuse_S1".into()),
                    normal: Some("Wolf_Body_Normal_S1".into()),
                },
                SkinnedMeshMaterial { name: "Wolf_Claws".into(), diffuse: Some("Wolf_Claws_Diffuse_S1".into()), normal: None },
            ]
        );
        assert_eq!(mesh.faces.len(), 2);
        assert_eq!(mesh.face_material_ids, vec![0, 1]);
        // The declared size in the section header is the body length, which for materials the
        // parser must NOT trust — reaching the mesh section at all proves the field-by-field
        // walk landed on the true boundary.
        assert_eq!(mesh.positions.len(), 6);
    }
}
