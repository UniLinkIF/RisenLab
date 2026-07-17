//! "Point at risen.exe (or its shortcut), we take it from there."
//!
//! Three steps: resolve a `.lnk` shortcut to its target if needed, walk up from the exe to
//! find the game root (the folder that has a `data/` directory holding archives), then walk
//! that `data/` tree to collect every `.pak`/patch-volume file — matching the naming
//! convention confirmed in `docs/p0x-patches.md` (`*.pak`, `*.p0?`, `*.0?`).

use std::io;
use std::path::{Path, PathBuf};

/// `LocalBasePath` (the non-Unicode variant) is encoded in the system's active ANSI codepage
/// at the time the shortcut was created, not UTF-8 — decoding it as UTF-8 corrupts any
/// non-ASCII path (confirmed on a real shortcut pointing through a Cyrillic folder name).
/// On the same machine the shortcut was made on (the common case for resolving a local
/// Desktop/Start Menu shortcut), the current process's ANSI codepage matches, so
/// `MultiByteToWideChar(CP_ACP, ...)` decodes it correctly.
#[cfg(windows)]
fn decode_ansi_path(bytes: &[u8]) -> String {
    // Manual FFI instead of the `windows-sys` crate: after a cache wipe that crate can't
    // rebuild in the dev sandbox (its generated import libs need binutils' dlltool/as),
    // while a plain extern declaration links against the libkernel32.a import lib that
    // ships with Rust's own self-contained gnu toolchain.
    const CP_ACP: u32 = 0;
    #[link(name = "kernel32")]
    extern "system" {
        fn MultiByteToWideChar(
            codepage: u32,
            flags: u32,
            bytes: *const u8,
            byte_len: i32,
            wide: *mut u16,
            wide_len: i32,
        ) -> i32;
    }
    unsafe {
        let wide_len =
            MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), bytes.len() as i32, std::ptr::null_mut(), 0);
        if wide_len <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide = vec![0u16; wide_len as usize];
        MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), bytes.len() as i32, wide.as_mut_ptr(), wide_len);
        String::from_utf16_lossy(&wide)
    }
}

#[cfg(not(windows))]
fn decode_ansi_path(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Resolves a Windows `.lnk` shortcut to its target path. If `path` isn't a `.lnk`, it's
/// returned unchanged (so callers can pass either a shortcut or a direct .exe path).
pub fn resolve_shortcut(path: &Path) -> io::Result<PathBuf> {
    let is_lnk = path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("lnk"))
        .unwrap_or(false);
    if !is_lnk {
        return Ok(path.to_path_buf());
    }
    let data = std::fs::read(path)?;
    parse_lnk_target(&data)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "could not read .lnk target path"))
}

/// Minimal MS-SHLLINK parser: only extracts LinkInfo's LocalBasePath, which is what a
/// shortcut to a local file (e.g. Desktop shortcut to risen.exe) actually stores. Doesn't
/// attempt the LinkTargetIDList or network-path cases.
fn parse_lnk_target(data: &[u8]) -> Option<String> {
    const HEADER_SIZE: usize = 76;
    const SHELL_LINK_CLSID: [u8; 16] = [
        0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x46,
    ];
    if data.len() < HEADER_SIZE {
        return None;
    }
    if data[0..4] != [0x4C, 0x00, 0x00, 0x00] || data[4..20] != SHELL_LINK_CLSID {
        return None; // not a shell link file
    }
    let link_flags = u32::from_le_bytes(data[20..24].try_into().ok()?);
    const HAS_LINK_INFO: u32 = 0x0000_0002;
    const HAS_LINK_TARGET_ID_LIST: u32 = 0x0000_0001;

    let mut offset = HEADER_SIZE;
    if link_flags & HAS_LINK_TARGET_ID_LIST != 0 {
        // IDList: u16 size, then that many bytes, then a terminal 2-byte 0x0000.
        let size = u16::from_le_bytes(data.get(offset..offset + 2)?.try_into().ok()?) as usize;
        offset += 2 + size;
    }
    if link_flags & HAS_LINK_INFO == 0 {
        return None;
    }

    let link_info_start = offset;
    let link_info_size = u32::from_le_bytes(data.get(offset..offset + 4)?.try_into().ok()?) as usize;
    let link_info = data.get(link_info_start..link_info_start + link_info_size)?;

    // LinkInfoHeaderSize at +4, LinkInfoFlags at +8, VolumeIDOffset at +12,
    // LocalBasePathOffset at +16 (LocalBasePathOffsetUnicode at +28..32 only present when
    // LinkInfoHeaderSize >= 0x24). Confirmed against a real Windows-generated .lnk: the
    // offset actually landed on a control byte from VolumeID until this was fixed to +16.
    let link_info_flags = u32::from_le_bytes(link_info.get(8..12)?.try_into().ok()?);
    const VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 0x1;
    if link_info_flags & VOLUME_ID_AND_LOCAL_BASE_PATH == 0 {
        return None;
    }
    let local_base_path_offset =
        u32::from_le_bytes(link_info.get(16..20)?.try_into().ok()?) as usize;
    let path_bytes = &link_info[local_base_path_offset..];
    let end = path_bytes.iter().position(|&b| b == 0)?;
    Some(decode_ansi_path(&path_bytes[..end]))
}

const ARCHIVE_SUBDIRS: &[&str] = &["compiled", "common"];

/// Given a path to (or inside) a Risen install, walk upward looking for a `data` directory
/// that actually contains archives — resilient to the exe living under `bin/`, `system/`, or
/// directly at the root in different Risen releases.
pub fn discover_game_root(exe_path: &Path) -> Option<PathBuf> {
    let mut dir = exe_path.parent()?;
    loop {
        let data_dir = dir.join("data");
        if data_dir.is_dir() && ARCHIVE_SUBDIRS.iter().any(|s| data_dir.join(s).is_dir()) {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

fn looks_like_archive(path: &Path) -> bool {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_ascii_lowercase(),
        None => return false,
    };
    if ext == "pak" {
        return true;
    }
    // .p0x / .pXX (e.g. p01, p02) or bare numeric .00 / .01
    let rest = ext.strip_prefix('p').unwrap_or(&ext);
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

#[derive(Debug, Clone)]
pub struct DiscoveredArchive {
    pub path: PathBuf,
    /// Which top-level data subfolder it came from ("compiled", "common", ...).
    pub group: String,
}

/// Recursively collects every archive file under `game_root/data/`.
pub fn discover_archives(game_root: &Path) -> io::Result<Vec<DiscoveredArchive>> {
    let data_dir = game_root.join("data");
    let mut out = Vec::new();
    for group in ARCHIVE_SUBDIRS {
        let dir = data_dir.join(group);
        if dir.is_dir() {
            walk_for_archives(&dir, group, &mut out)?;
        }
    }
    Ok(out)
}

fn walk_for_archives(dir: &Path, group: &str, out: &mut Vec<DiscoveredArchive>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_for_archives(&path, group, out)?;
        } else if looks_like_archive(&path) {
            out.push(DiscoveredArchive {
                path,
                group: group.to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a synthetic but spec-accurate `.lnk` buffer: header + a minimal LinkInfo
    /// (no `LinkTargetIDList`, matching real MS-SHLLINK field offsets) pointing at
    /// `target_path`. Regression fixture for a real bug: the parser originally read
    /// `LocalBasePathOffset` from struct offset +12 (actually `VolumeIDOffset`) instead of
    /// +16, discovered by testing against a real Windows-generated shortcut.
    fn synthetic_lnk(target_path: &str) -> Vec<u8> {
        const SHELL_LINK_CLSID: [u8; 16] = [
            0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x46,
        ];
        let mut buf = vec![0u8; 76];
        buf[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
        buf[4..20].copy_from_slice(&SHELL_LINK_CLSID);
        const HAS_LINK_INFO: u32 = 0x0000_0002;
        buf[20..24].copy_from_slice(&HAS_LINK_INFO.to_le_bytes());

        let mut path_bytes = target_path.as_bytes().to_vec();
        path_bytes.push(0);
        let local_base_path_offset: u32 = 28; // right after the 28-byte LinkInfoHeader
        let link_info_size = 28 + path_bytes.len() as u32;

        buf.extend_from_slice(&link_info_size.to_le_bytes()); // LinkInfoSize
        buf.extend_from_slice(&28u32.to_le_bytes()); // LinkInfoHeaderSize
        buf.extend_from_slice(&1u32.to_le_bytes()); // LinkInfoFlags: VOLUME_ID_AND_LOCAL_BASE_PATH
        buf.extend_from_slice(&28u32.to_le_bytes()); // VolumeIDOffset (unused by parser)
        buf.extend_from_slice(&local_base_path_offset.to_le_bytes()); // LocalBasePathOffset
        buf.extend_from_slice(&0u32.to_le_bytes()); // CommonNetworkRelativeLinkOffset
        buf.extend_from_slice(&0u32.to_le_bytes()); // CommonPathSuffixOffset
        buf.extend_from_slice(&path_bytes);
        buf
    }

    #[test]
    fn parses_local_base_path_from_synthetic_link_info() {
        let data = synthetic_lnk(r"C:\Games\Risen\bin\Risen.exe");
        let target = parse_lnk_target(&data).expect("should parse a target path");
        assert_eq!(target, r"C:\Games\Risen\bin\Risen.exe");
    }

    #[test]
    fn recognizes_archive_extensions() {
        assert!(looks_like_archive(Path::new("images.pak")));
        assert!(looks_like_archive(Path::new("images.p01")));
        assert!(looks_like_archive(Path::new("images.00")));
        assert!(!looks_like_archive(Path::new("readme.txt")));
        assert!(!looks_like_archive(Path::new("risen.exe")));
    }

    #[test]
    fn discovers_root_and_archives_from_a_synthetic_install() {
        let tmp = std::env::temp_dir().join(format!("risenlab_test_{}", std::process::id()));
        let bin_dir = tmp.join("bin");
        let compiled_dir = tmp.join("data").join("compiled");
        let common_dir = tmp.join("data").join("common");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::create_dir_all(&compiled_dir).unwrap();
        std::fs::create_dir_all(&common_dir).unwrap();
        std::fs::write(compiled_dir.join("images.pak"), b"x").unwrap();
        std::fs::write(compiled_dir.join("images.p01"), b"x").unwrap();
        std::fs::write(common_dir.join("materials.pak"), b"x").unwrap();
        let exe = bin_dir.join("Risen.exe");
        std::fs::write(&exe, b"x").unwrap();

        let root = discover_game_root(&exe).expect("should find game root");
        assert_eq!(root, tmp);

        let archives = discover_archives(&root).unwrap();
        assert_eq!(archives.len(), 3);
        assert!(archives.iter().any(|a| a.path.ends_with("images.pak") && a.group == "compiled"));
        assert!(archives.iter().any(|a| a.path.ends_with("materials.pak") && a.group == "common"));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
