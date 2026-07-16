//! Risen 1's legacy `._xmot` motion-clip format — the older EMotionFX "XSM " sub-format,
//! distinct from both the modern O3DE-donated EMotionFX source and Gothic 3's `g3blend`
//! addon (neither's magic bytes or field layout match this file; see
//! `docs/formats/xmot.md` for the full reverse-engineering trail). This module is an
//! independent implementation, empirically validated against real Wolf animation clips.
//!
//! **Per-bone record layout** (found empirically; the header attribution below was finally
//! pinned down on the real Ogre walk clip, byte-exact-validated across records):
//! `[132-byte header][u32 name_len][name][position keys][rotation keys][scale keys][scale-rotation keys]`
//! - the 132-byte header PRECEDES its own bone's name (an earlier reading treated it as a
//!   trailing block of the previous record — same bytes, wrong owner) and holds the
//!   submotion's `poseRot`/`bindPoseRot`/`poseScaleRot`/`bindScaleRot` quaternions and
//!   `posePos`/`poseScale`/`bindPos`/`bindScale` vectors (4 quats + 4 vec3 = 112 bytes),
//!   then **four `u32` per-channel key counts at header offsets 112/116/120/124**:
//!   `numPosKeys`, `numRotKeys`, `numScaleKeys`, `numScaleRotKeys`, then one more 4-byte
//!   field of unknown meaning (128..132).
//! - a position/scale key is `[f32 x, f32 y, f32 z, f32 time]` (16 bytes)
//! - a rotation/scale-rotation key is `[f32 x, f32 y, f32 z, f32 w, f32 time]` (20 bytes,
//!   unit quaternion)
//!
//! **Why the counts matter (a real, owner-visible bug)**: an earlier version had no counts
//! and detected channels by scanning while `time` restarts at 0 and strictly increases. That
//! misreads every bone whose ROTATION channel is empty but whose SCALE-ROTATION channel is
//! not — both are 20-byte quat+time streams, so the scan swallowed the scale-rotation keys
//! (near-identity wobble quats) as the bone's rotation track. On the real Ogre walk clip the
//! hip/teeth/hand bones have exactly that shape (`numRotKeys == 0`, `numScaleRotKeys == 36`),
//! so both hips got near-identity local rotations instead of their ~97°-from-identity bind
//! rotation — folding the legs up into the body ("в анімації ламаються САМЕ НОГИ").
//! The constant `131`-valued field noted earlier as a red herring was in fact the Wolf clip's
//! `numScaleRotKeys` — it recurred on every bone because every bone had a full scale-rotation
//! channel, while rotation counts varied.
//!
//! The old time-step scan is kept as a **fallback** for records where no plausible header is
//! found (defensive against not-yet-seen clip variants; validated identical on all clips
//! where counts are present).

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

/// Offset of the four per-channel `u32` key counts inside the 132-byte record header.
const HEADER_KEY_COUNTS_OFFSET: usize = 112;
/// Sanity cap on a single channel's declared key count — real clips top out at a few hundred
/// keys (30fps × clip seconds); anything huge means we're not looking at a real header.
const MAX_PLAUSIBLE_KEYS: u32 = 100_000;

fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    data.get(off..off + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// The four per-channel key counts (`numPos`, `numRot`, `numScale`, `numScaleRot`) from the
/// 132-byte header that immediately precedes `name_off`, or `None` when there's no room for a
/// header or the values are implausible (→ caller falls back to time-step scanning).
fn read_header_key_counts(data: &[u8], name_off: usize) -> Option<[u32; 4]> {
    let header_off = name_off.checked_sub(TAIL_BLOCK_LEN)?;
    let mut counts = [0u32; 4];
    for (i, c) in counts.iter_mut().enumerate() {
        let v = read_u32(data, header_off + HEADER_KEY_COUNTS_OFFSET + i * 4)?;
        if v > MAX_PLAUSIBLE_KEYS {
            return None;
        }
        *c = v;
    }
    Some(counts)
}

fn read_vec3_time_keys(data: &[u8], start: usize, count: u32) -> Option<(Vec<[f32; 4]>, usize)> {
    let mut keys = Vec::with_capacity(count as usize);
    let mut off = start;
    for _ in 0..count {
        keys.push([read_f32(data, off)?, read_f32(data, off + 4)?, read_f32(data, off + 8)?, read_f32(data, off + 12)?]);
        off += 16;
    }
    Some((keys, off - start))
}

fn read_quat_time_keys(data: &[u8], start: usize, count: u32) -> Option<(Vec<[f32; 5]>, usize)> {
    let mut keys = Vec::with_capacity(count as usize);
    let mut off = start;
    for _ in 0..count {
        keys.push([
            read_f32(data, off)?,
            read_f32(data, off + 4)?,
            read_f32(data, off + 8)?,
            read_f32(data, off + 12)?,
            read_f32(data, off + 16)?,
        ]);
        off += 20;
    }
    Some((keys, off - start))
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

        // Preferred path: exact per-channel key counts from the record's own 132-byte header.
        // This is what correctly distinguishes an empty ROTATION channel followed by a full
        // SCALE-ROTATION channel (both are 20-byte quat+time streams) — see the module docs
        // for the real Ogre legs bug this fixes.
        if let Some([num_pos, num_rot, num_scale, _num_scale_rot]) = read_header_key_counts(data, name_off) {
            if let Some((position_keys, pos_len)) = read_vec3_time_keys(data, body_start, num_pos) {
                if let Some((rotation_keys, rot_len)) = read_quat_time_keys(data, body_start + pos_len, num_rot) {
                    if let Some((scale_keys, _)) = read_vec3_time_keys(data, body_start + pos_len + rot_len, num_scale) {
                        // scale-rotation keys follow, deliberately unused for playback
                        out.push(BoneMotion { bone_name: bone_name.clone(), position_keys, rotation_keys, scale_keys });
                        continue;
                    }
                }
            }
        }

        // Fallback: the original time-step scan (kept for clip variants without a readable
        // header). Its known limitation: a bone with scale-rotation keys but no rotation keys
        // gets the scale-rotation stream misread as its rotation track.
        let (position_keys, pos_len) = scan_vec3_time_track(data, body_start, data.len());
        let (rotation_keys, rot_len) = scan_quat_time_track(data, body_start + pos_len, data.len());
        let (scale_keys, _) = scan_vec3_time_track(data, body_start + pos_len + rot_len, data.len());
        out.push(BoneMotion { bone_name: bone_name.clone(), position_keys, rotation_keys, scale_keys });
    }
    Ok(out)
}

/// Byte locations of one bone's keyframe channels inside a raw `._xmot` file — the write-side
/// counterpart of `parse_motion`, for patching key VALUES in place. Only records whose header
/// counts are readable are located (same rule as the parse side's preferred path); in-place
/// patching never changes counts/sizes, so the file structure — including every not-yet-decoded
/// wrapper/chunk field — is preserved byte-for-byte.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordLocation {
    pub bone_name: String,
    /// Absolute byte offset of the first position key; `num_pos` 16-byte keys follow.
    pub pos_off: usize,
    pub num_pos: u32,
    /// Absolute byte offset of the first rotation key; `num_rot` 20-byte keys follow.
    pub rot_off: usize,
    pub num_rot: u32,
}

/// Locates the patchable keyframe channels for every requested bone (bones without a readable
/// counts-header are skipped — they can't be patched safely).
pub fn locate_records(data: &[u8], bone_names: &[String]) -> Result<Vec<RecordLocation>> {
    let Some(xsm_off) = data.windows(4).position(|w| w == b"XSM ") else {
        bail!("xmot: not a recognized legacy motion clip (no 'XSM ' magic found)");
    };
    let payload_start = xsm_off + 8;
    let mut out = Vec::new();
    for bone_name in bone_names {
        let Some(name_off) = find_length_prefixed_name(data, bone_name, payload_start) else { continue };
        let Some([num_pos, num_rot, _num_scale, _num_scale_rot]) = read_header_key_counts(data, name_off) else {
            continue;
        };
        let pos_off = name_off + 4 + bone_name.len();
        let rot_off = pos_off + num_pos as usize * 16;
        // Bounds check the full extent so a truncated file can't cause a partial patch.
        if rot_off + num_rot as usize * 20 > data.len() {
            bail!("xmot: record for '{bone_name}' extends past end of file");
        }
        out.push(RecordLocation { bone_name: bone_name.clone(), pos_off, num_pos, rot_off, num_rot });
    }
    Ok(out)
}

/// Returns a copy of the raw `._xmot` with the given bones' keyframe VALUES replaced in place.
/// Each supplied `BoneMotion`'s key counts must exactly match the file's (same clip, values
/// edited — e.g. by `smooth_tracks`); anything else is an error, since growing/shrinking a
/// record would require rewriting chunk sizes whose semantics aren't fully decoded. Times are
/// written too (same count ⇒ same byte layout), so retiming within a fixed key count also works.
pub fn patch_motion_keys(data: &[u8], edits: &[BoneMotion]) -> Result<Vec<u8>> {
    let names: Vec<String> = edits.iter().map(|e| e.bone_name.clone()).collect();
    let locations = locate_records(data, &names)?;
    let mut out = data.to_vec();
    for edit in edits {
        let Some(loc) = locations.iter().find(|l| l.bone_name == edit.bone_name) else {
            // A bone the clip doesn't animate (or whose header is unreadable) with an empty
            // edit is a no-op; with real keys it's a caller bug worth failing loudly on.
            if edit.position_keys.is_empty() && edit.rotation_keys.is_empty() {
                continue;
            }
            bail!("xmot: bone '{}' not found/patchable in this clip", edit.bone_name);
        };
        if edit.position_keys.len() != loc.num_pos as usize {
            bail!(
                "xmot: '{}' position key count {} != file's {} (in-place patch can't resize)",
                edit.bone_name,
                edit.position_keys.len(),
                loc.num_pos
            );
        }
        if edit.rotation_keys.len() != loc.num_rot as usize {
            bail!(
                "xmot: '{}' rotation key count {} != file's {} (in-place patch can't resize)",
                edit.bone_name,
                edit.rotation_keys.len(),
                loc.num_rot
            );
        }
        for (i, key) in edit.position_keys.iter().enumerate() {
            let off = loc.pos_off + i * 16;
            for (j, v) in key.iter().enumerate() {
                out[off + j * 4..off + j * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        for (i, key) in edit.rotation_keys.iter().enumerate() {
            let off = loc.rot_off + i * 20;
            for (j, v) in key.iter().enumerate() {
                out[off + j * 4..off + j * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    Ok(out)
}

/// Real, local (no external AI) motion cleanup: a gentle low-pass filter over each bone's
/// keyframe tracks — the "згладжування дрижання" direction from `docs/AI.md`. Each interior
/// key is blended toward the midpoint of its neighbors by `strength` (0.0 = untouched,
/// 1.0 = fully averaged); first/last keys are kept exactly so loop boundaries stay seamless.
/// Quaternions are hemisphere-aligned before blending (q and -q are the same rotation — naive
/// averaging across the double-cover boundary would swing through a huge wrong arc) and
/// re-normalized after.
pub fn smooth_tracks(tracks: &[BoneMotion], strength: f32) -> Vec<BoneMotion> {
    let s = strength.clamp(0.0, 1.0);
    if s == 0.0 {
        // Bit-exact no-op — even renormalizing an untouched quaternion would flip low bits
        // (real files store not-perfectly-unit floats), breaking the byte-identical
        // round-trip guarantee the write chain is verified with.
        return tracks.to_vec();
    }
    tracks
        .iter()
        .map(|t| {
            let mut out = t.clone();
            for i in 1..t.position_keys.len().saturating_sub(1) {
                let (prev, cur, next) = (t.position_keys[i - 1], t.position_keys[i], t.position_keys[i + 1]);
                for c in 0..3 {
                    let mid = (prev[c] + next[c]) * 0.5;
                    out.position_keys[i][c] = cur[c] + (mid - cur[c]) * s;
                }
                // time (index 3) untouched
            }
            for i in 1..t.rotation_keys.len().saturating_sub(1) {
                let cur = t.rotation_keys[i];
                let mut prev = t.rotation_keys[i - 1];
                let mut next = t.rotation_keys[i + 1];
                let align = |q: &mut [f32; 5], reference: &[f32; 5]| {
                    let dot: f32 = q[..4].iter().zip(&reference[..4]).map(|(a, b)| a * b).sum();
                    if dot < 0.0 {
                        for v in q[..4].iter_mut() {
                            *v = -*v;
                        }
                    }
                };
                align(&mut prev, &cur);
                align(&mut next, &cur);
                let mut blended = [0.0f32; 4];
                for c in 0..4 {
                    let mid = (prev[c] + next[c]) * 0.5;
                    blended[c] = cur[c] + (mid - cur[c]) * s;
                }
                let norm = blended.iter().map(|v| v * v).sum::<f32>().sqrt();
                if norm > 1e-6 {
                    for c in 0..4 {
                        out.rotation_keys[i][c] = blended[c] / norm;
                    }
                }
                // time (index 4) untouched
            }
            out
        })
        .collect()
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

    /// A real-shaped 132-byte header: 112 bytes of pose/bind transform data, then the four
    /// per-channel key counts (pos/rot/scale/scaleRot), then 4 unknown bytes.
    fn push_header_with_counts(buf: &mut Vec<u8>, num_pos: u32, num_rot: u32, num_scale: u32, num_scale_rot: u32) {
        for _ in 0..(HEADER_KEY_COUNTS_OFFSET / 4) {
            buf.extend_from_slice(&0.5f32.to_le_bytes());
        }
        for c in [num_pos, num_rot, num_scale, num_scale_rot] {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        buf.extend_from_slice(&[0u8; 4]);
    }

    /// The exact Ogre-legs shape: a bone whose ROTATION channel is empty but whose
    /// SCALE-ROTATION channel has keys. The scale-rotation stream is byte-identical in shape
    /// to a rotation stream (quat + time), so only the header's counts can tell them apart —
    /// misreading it as the rotation track is what folded the Ogre's legs up into its body.
    #[test]
    fn scale_rotation_keys_are_not_mistaken_for_rotation_keys() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"XSM ");
        buf.extend_from_slice(&[1, 1, 0, 0]);
        push_header_with_counts(&mut buf, 0, 0, 0, 2);
        push_name(&mut buf, "Bone_Left_Leg_Hip_1");
        // scale-rotation channel only: near-identity wobble quats, times from 0
        push_rot_key(&mut buf, 0.001, 0.002, -0.03, 0.9995, 0.0);
        push_rot_key(&mut buf, -0.002, 0.001, 0.01, 0.9998, 0.04);
        let result = parse_motion(&buf, &["Bone_Left_Leg_Hip_1".to_string()]).unwrap();
        assert!(result[0].rotation_keys.is_empty(), "scale-rotation keys must not become the rotation track");
        assert!(result[0].position_keys.is_empty());
        assert!(result[0].scale_keys.is_empty());
    }

    /// Builds a counts-header record file with one bone: 2 position keys + 3 rotation keys.
    fn synthetic_xmot_with_header(name: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"XSM ");
        buf.extend_from_slice(&[1, 1, 0, 0]);
        push_header_with_counts(&mut buf, 2, 3, 0, 0);
        push_name(&mut buf, name);
        push_pos_key(&mut buf, 1.0, 2.0, 3.0, 0.0);
        push_pos_key(&mut buf, 1.5, 2.5, 3.5, 0.04);
        push_rot_key(&mut buf, 0.0, 0.0, 0.0, 1.0, 0.0);
        push_rot_key(&mut buf, 0.1, 0.0, 0.0, 0.99499, 0.04);
        push_rot_key(&mut buf, 0.0, 0.0, 0.0, 1.0, 0.08);
        buf
    }

    #[test]
    fn patching_with_identical_values_is_byte_identical() {
        let data = synthetic_xmot_with_header("Bone_ROOT");
        let parsed = parse_motion(&data, &["Bone_ROOT".to_string()]).unwrap();
        let patched = patch_motion_keys(&data, &parsed).unwrap();
        assert_eq!(patched, data, "round trip must reproduce the file byte-for-byte");
    }

    #[test]
    fn patched_values_read_back_and_rest_of_file_is_untouched() {
        let data = synthetic_xmot_with_header("Bone_ROOT");
        let mut edit = parse_motion(&data, &["Bone_ROOT".to_string()]).unwrap();
        edit[0].position_keys[1] = [9.0, 8.0, 7.0, 0.04];
        edit[0].rotation_keys[1] = [0.2, 0.0, 0.0, 0.9798, 0.04];
        let patched = patch_motion_keys(&data, &edit).unwrap();
        assert_eq!(patched.len(), data.len());
        let reparsed = parse_motion(&patched, &["Bone_ROOT".to_string()]).unwrap();
        assert_eq!(reparsed[0].position_keys[1], [9.0, 8.0, 7.0, 0.04]);
        assert_eq!(reparsed[0].rotation_keys[1], [0.2, 0.0, 0.0, 0.9798, 0.04]);
        // everything before the first position key (header + name) is untouched
        let keys_start = patched.len() - (2 * 16 + 3 * 20);
        assert_eq!(&patched[..keys_start], &data[..keys_start]);
    }

    #[test]
    fn patching_with_wrong_key_count_is_rejected() {
        let data = synthetic_xmot_with_header("Bone_ROOT");
        let mut edit = parse_motion(&data, &["Bone_ROOT".to_string()]).unwrap();
        edit[0].rotation_keys.pop();
        assert!(patch_motion_keys(&data, &edit).is_err());
    }

    #[test]
    fn smoothing_attenuates_a_spike_and_keeps_endpoints_and_times() {
        let track = BoneMotion {
            bone_name: "Bone_Spine".into(),
            position_keys: vec![
                [0.0, 0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0, 0.04], // spike
                [0.0, 0.0, 0.0, 0.08],
            ],
            rotation_keys: vec![
                [0.0, 0.0, 0.0, 1.0, 0.0],
                [0.5, 0.0, 0.0, 0.8660, 0.04], // 60° jerk between identity neighbors
                [0.0, 0.0, 0.0, 1.0, 0.08],
            ],
            scale_keys: vec![],
        };
        let smoothed = &smooth_tracks(&[track.clone()], 0.5)[0];
        assert_eq!(smoothed.position_keys[0], track.position_keys[0]);
        assert_eq!(smoothed.position_keys[2], track.position_keys[2]);
        assert!((smoothed.position_keys[1][0] - 5.0).abs() < 1e-5, "spike halves at strength 0.5");
        assert_eq!(smoothed.position_keys[1][3], 0.04, "time untouched");
        let q = &smoothed.rotation_keys[1];
        assert!(q[0] < 0.5, "rotation jerk attenuated: x was 0.5, now {}", q[0]);
        let norm: f32 = q[..4].iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "stays a unit quaternion");
        assert_eq!(q[4], 0.04, "time untouched");
    }

    #[test]
    fn smoothing_at_zero_strength_changes_nothing_and_handles_negated_quats() {
        let track = BoneMotion {
            bone_name: "B".into(),
            position_keys: vec![],
            rotation_keys: vec![
                [0.0, 0.0, 0.0, 1.0, 0.0],
                [0.01, 0.0, 0.0, 0.99995, 0.04],
                // same rotation as identity but sign-negated (double cover) — naive averaging
                // would swing wildly; hemisphere alignment must keep this stable
                [-0.0, -0.0, -0.0, -1.0, 0.08],
            ],
            scale_keys: vec![],
        };
        let zero = &smooth_tracks(&[track.clone()], 0.0)[0];
        assert_eq!(zero.rotation_keys, track.rotation_keys);
        let smoothed = &smooth_tracks(&[track], 1.0)[0];
        let q = &smoothed.rotation_keys[1];
        // midpoint of identity and (negated) identity is identity — x goes toward 0
        assert!(q[0].abs() < 0.01, "hemisphere-aligned midpoint stays near identity, got x={}", q[0]);
        assert!(q[3].abs() > 0.999);
    }

    #[test]
    fn header_key_counts_split_rotation_from_scale_rotation() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"XSM ");
        buf.extend_from_slice(&[1, 1, 0, 0]);
        push_header_with_counts(&mut buf, 1, 2, 0, 1);
        push_name(&mut buf, "Bone_Left_Leg_Leg_1");
        push_pos_key(&mut buf, 19.9, 0.0, 0.0, 0.0);
        // real rotation channel (big quats)
        push_rot_key(&mut buf, 0.43, -0.29, 0.57, 0.64, 0.0);
        push_rot_key(&mut buf, 0.48, -0.34, 0.53, 0.62, 0.04);
        // scale-rotation channel (near identity) — must be excluded
        push_rot_key(&mut buf, 0.005, 0.02, -0.002, 0.9998, 0.0);
        let result = parse_motion(&buf, &["Bone_Left_Leg_Leg_1".to_string()]).unwrap();
        assert_eq!(result[0].position_keys, vec![[19.9, 0.0, 0.0, 0.0]]);
        assert_eq!(result[0].rotation_keys.len(), 2);
        assert_eq!(result[0].rotation_keys[0], [0.43, -0.29, 0.57, 0.64, 0.0]);
        assert_eq!(result[0].rotation_keys[1], [0.48, -0.34, 0.53, 0.62, 0.04]);
        assert!(result[0].scale_keys.is_empty());
    }
}
