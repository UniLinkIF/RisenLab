//! Risen 1's legacy `._xmot` motion-clip format — the older EMotionFX "XSM " sub-format,
//! distinct from both the modern O3DE-donated EMotionFX source and Gothic 3's `g3blend`
//! addon (neither's magic bytes or field layout match this file; see
//! `docs/formats/xmot.md` for the full reverse-engineering trail). This module is an
//! independent implementation, empirically validated against real Wolf animation clips.
//!
//! **Per-bone record layout** (found empirically, byte-exact-validated: after scanning a
//! bone's position/rotation/scale channels, the next 132 bytes are always a fixed-size
//! trailing block, landing byte-exact on the *next* bone's own name, across every real bone
//! tested — animated and static alike):
//! `[u32 name_len][name][position keys][rotation keys][scale keys][132-byte trailing block]`
//! - a position/scale key is `[f32 x, f32 y, f32 z, f32 time]` (16 bytes)
//! - a rotation key is `[f32 x, f32 y, f32 z, f32 w, f32 time]` (20 bytes, unit quaternion)
//! - the 132-byte trailing block holds `poseRot`/`bindPoseRot`/`poseScaleRot`/
//!   `bindPoseScaleRot` quaternions and `pos`/`scale`/`bindPos`/`bindScale` vectors (matching
//!   4 quats + 4 vec3 = 112 bytes) plus a handful of trailing fields whose exact meaning
//!   isn't decoded yet (not needed for playback — the equivalent bind-pose data already
//!   comes from the matching `.xmac` actor via `xmac::parse_skeleton`)
//!
//! **Why this scans instead of trusting an upfront key count**: the surrounding chunk
//! framing (`"GR01"`/`"MO01"` wrapper, `MOTION_CHUNK_INFO`/`MOTION_CHUNK_SUBMOTIONS`) is only
//! partially mapped — in particular, no field was ever found that reliably holds a given
//! bone's real key count (a constant `131`-valued field recurs in the trailing block on
//! *every* bone regardless of its real key count, so it isn't that). What's fully reliable
//! instead is that each channel's first real key always starts at `time == 0.0`, and every
//! following key's time strictly increases by a small step — so a channel's real length is
//! determined by scanning until that pattern breaks, not by reading a count.

use anyhow::{bail, Result};
use serde::Serialize;

const TAIL_BLOCK_LEN: usize = 132;
/// A key's `time` step must be positive and below this to count as a real continuation —
/// generous enough to tolerate frame rates below the 30fps seen in every real sample so far.
const MAX_KEY_DELTA_SECONDS: f32 = 0.5;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoneMotion {
    pub bone_name: String,
    /// `[x, y, z, time]`, empty if this clip doesn't animate the bone's position.
    pub position_keys: Vec<[f32; 4]>,
    /// `[x, y, z, w, time]`, empty if this clip doesn't animate the bone's rotation.
    pub rotation_keys: Vec<[f32; 5]>,
    /// `[x, y, z, time]`, empty if this clip doesn't animate the bone's scale.
    pub scale_keys: Vec<[f32; 4]>,
}

impl BoneMotion {
    pub fn duration(&self) -> f32 {
        let mut d: f32 = 0.0;
        if let Some(k) = self.position_keys.last() {
            d = d.max(k[3]);
        }
        if let Some(k) = self.rotation_keys.last() {
            d = d.max(k[4]);
        }
        if let Some(k) = self.scale_keys.last() {
            d = d.max(k[3]);
        }
        d
    }
}

fn read_f32(data: &[u8], off: usize) -> Option<f32> {
    data.get(off..off + 4).map(|b| f32::from_le_bytes(b.try_into().unwrap()))
}

/// A real channel's first key is always at `time == 0`; every following key's time strictly
/// increases by a plausible small step. Returns `true` to keep the candidate, `false` to stop
/// the scan without consuming it.
fn accept_time_step(prev_t: Option<f32>, t: f32) -> bool {
    match prev_t {
        None => t.abs() < 1e-5,
        Some(p) => {
            let dt = t - p;
            dt > 0.0 && dt < MAX_KEY_DELTA_SECONDS
        }
    }
}

fn scan_vec3_time_track(data: &[u8], start: usize, end: usize) -> (Vec<[f32; 4]>, usize) {
    let mut keys = Vec::new();
    let mut off = start;
    let mut prev_t: Option<f32> = None;
    while off + 16 <= end {
        let (Some(x), Some(y), Some(z), Some(t)) =
            (read_f32(data, off), read_f32(data, off + 4), read_f32(data, off + 8), read_f32(data, off + 12))
        else {
            break;
        };
        if !accept_time_step(prev_t, t) || x.abs() > 1.0e6 || y.abs() > 1.0e6 || z.abs() > 1.0e6 {
            break;
        }
        keys.push([x, y, z, t]);
        prev_t = Some(t);
        off += 16;
    }
    (keys, off - start)
}

fn scan_quat_time_track(data: &[u8], start: usize, end: usize) -> (Vec<[f32; 5]>, usize) {
    let mut keys = Vec::new();
    let mut off = start;
    let mut prev_t: Option<f32> = None;
    while off + 20 <= end {
        let (Some(x), Some(y), Some(z), Some(w), Some(t)) = (
            read_f32(data, off),
            read_f32(data, off + 4),
            read_f32(data, off + 8),
            read_f32(data, off + 12),
            read_f32(data, off + 16),
        ) else {
            break;
        };
        let magnitude = (x * x + y * y + z * z + w * w).sqrt();
        if !(0.9..1.1).contains(&magnitude) || !accept_time_step(prev_t, t) {
            break;
        }
        keys.push([x, y, z, w, t]);
        prev_t = Some(t);
        off += 20;
    }
    (keys, off - start)
}

/// Finds `name` on disk as it's really stored: a little-endian `u32` length immediately
/// followed by the raw ASCII bytes (no terminator) — the same length-prefixed-string
/// convention `.xmac` uses for node names. Returns the offset of the length prefix.
fn find_length_prefixed_name(data: &[u8], name: &str, from_offset: usize) -> Option<usize> {
    let needle = name.as_bytes();
    if needle.is_empty() || from_offset >= data.len() {
        return None;
    }
    let len_bytes = (needle.len() as u32).to_le_bytes();
    let haystack = &data[from_offset..];
    let mut search_from = 0usize;
    while search_from + needle.len() <= haystack.len() {
        let rel = haystack[search_from..].windows(needle.len()).position(|w| w == needle)?;
        let name_off = search_from + rel;
        if name_off >= 4 && haystack[name_off - 4..name_off] == len_bytes {
            return Some(from_offset + name_off - 4);
        }
        search_from = name_off + 1;
    }
    None
}

/// Parses the real per-bone position/rotation/scale keyframe tracks out of a decompressed
/// `._xmot` motion clip, one per name in `bone_names` (typically every bone in the matching
/// `.xmac` actor's skeleton — a bone this clip doesn't animate simply comes back with all
/// three tracks empty, not an error; only a clip that isn't recognizable at all is an error).
pub fn parse_motion(data: &[u8], bone_names: &[String]) -> Result<Vec<BoneMotion>> {
    let Some(xsm_off) = data.windows(4).position(|w| w == b"XSM ") else {
        bail!("xmot: not a recognized legacy motion clip (no 'XSM ' magic found)");
    };
    let payload_start = xsm_off + 8; // "XSM " + hi-version + lo-version + endian-type + padding

    let mut out = Vec::with_capacity(bone_names.len());
    for bone_name in bone_names {
        let Some(name_off) = find_length_prefixed_name(data, bone_name, payload_start) else {
            out.push(BoneMotion { bone_name: bone_name.clone(), ..Default::default() });
            continue;
        };
        let body_start = name_off + 4 + bone_name.len();
        let (position_keys, pos_len) = scan_vec3_time_track(data, body_start, data.len());
        let (rotation_keys, rot_len) = scan_quat_time_track(data, body_start + pos_len, data.len());
        let (scale_keys, scale_len) = scan_vec3_time_track(data, body_start + pos_len + rot_len, data.len());
        let _ = TAIL_BLOCK_LEN; // documents the expected trailing block; not read — see module docs
        let _record_end = body_start + pos_len + rot_len + scale_len + TAIL_BLOCK_LEN;
        out.push(BoneMotion { bone_name: bone_name.clone(), position_keys, rotation_keys, scale_keys });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_name(buf: &mut Vec<u8>, name: &str) {
        buf.extend_from_slice(&(name.len() as u32).to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
    }

    fn push_pos_key(buf: &mut Vec<u8>, x: f32, y: f32, z: f32, t: f32) {
        for v in [x, y, z, t] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    fn push_rot_key(buf: &mut Vec<u8>, x: f32, y: f32, z: f32, w: f32, t: f32) {
        for v in [x, y, z, w, t] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    /// A real trailing block always has a non-zero value where a following channel scan
    /// would look for "time" — real files never regress to `0.0` there, so this uses `1.0`
    /// throughout, which safely fails every scan's `time == 0` gate for the *next* bone.
    fn push_tail_block(buf: &mut Vec<u8>) {
        for _ in 0..(TAIL_BLOCK_LEN / 4) {
            buf.extend_from_slice(&1.0f32.to_le_bytes());
        }
    }

    fn synthetic_xmot(bones: &[(&str, &dyn Fn(&mut Vec<u8>))]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"some preamble bytes before the motion payload starts");
        buf.extend_from_slice(b"XSM ");
        buf.extend_from_slice(&[1, 1, 0, 0]); // hi version, lo version, endian type, padding
        for (name, push_body) in bones {
            push_name(&mut buf, name);
            push_body(&mut buf);
            push_tail_block(&mut buf);
        }
        buf
    }

    #[test]
    fn rejects_data_with_no_xsm_magic() {
        assert!(parse_motion(b"not a motion clip at all", &["Bone_ROOT".to_string()]).is_err());
    }

    #[test]
    fn a_static_bone_with_zero_keys_comes_back_with_empty_tracks() {
        let data = synthetic_xmot(&[("Bone_ROOT", &(|_: &mut Vec<u8>| {}))]);
        let result = parse_motion(&data, &["Bone_ROOT".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].position_keys.is_empty());
        assert!(result[0].rotation_keys.is_empty());
        assert!(result[0].scale_keys.is_empty());
    }

    #[test]
    fn an_animated_bone_s_position_and_rotation_keys_are_extracted_in_order() {
        let push_body = |buf: &mut Vec<u8>| {
            push_pos_key(buf, -3.622, 101.295, 42.907, 0.0);
            push_pos_key(buf, -3.565, 100.732, 42.987, 0.033);
            push_pos_key(buf, -3.424, 99.2, 43.187, 0.067);
            push_rot_key(buf, 0.0, 0.0, 0.0, 1.0, 0.0);
            push_rot_key(buf, 0.01, 0.0, 0.0, 0.99995, 0.033);
        };
        let data = synthetic_xmot(&[("Bone_Spine1", &push_body)]);
        let result = parse_motion(&data, &["Bone_Spine1".to_string()]).unwrap();
        assert_eq!(result[0].position_keys.len(), 3);
        assert_eq!(result[0].position_keys[0], [-3.622, 101.295, 42.907, 0.0]);
        assert_eq!(result[0].position_keys[2], [-3.424, 99.2, 43.187, 0.067]);
        assert_eq!(result[0].rotation_keys.len(), 2);
        assert!(result[0].scale_keys.is_empty());
        assert!((result[0].duration() - 0.067).abs() < 1e-6);
    }

    #[test]
    fn a_bone_name_not_present_in_the_clip_comes_back_empty_not_an_error() {
        let data = synthetic_xmot(&[("Bone_ROOT", &(|_: &mut Vec<u8>| {}))]);
        let result = parse_motion(&data, &["Bone_NeverAnimated".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].position_keys.is_empty());
    }

    #[test]
    fn multiple_bones_are_each_found_independently_of_file_order() {
        let push_a = |buf: &mut Vec<u8>| push_pos_key(buf, 1.0, 2.0, 3.0, 0.0);
        let data = synthetic_xmot(&[("Bone_Head", &push_a), ("Bone_ROOT", &(|_: &mut Vec<u8>| {}))]);
        // ask for them in the opposite order from how they're stored on disk
        let result = parse_motion(&data, &["Bone_ROOT".to_string(), "Bone_Head".to_string()]).unwrap();
        assert_eq!(result[0].bone_name, "Bone_ROOT");
        assert!(result[0].position_keys.is_empty());
        assert_eq!(result[1].bone_name, "Bone_Head");
        assert_eq!(result[1].position_keys, vec![[1.0, 2.0, 3.0, 0.0]]);
    }
}
