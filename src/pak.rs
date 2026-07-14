//! Risen 1 `.pak` container format.
//!
//! Format documented by Nico Bendlin (RisenPAK.txt, 2009-2011), verified byte-for-byte
//! against real `library.pak` / `materials.pak` files from a licensed Risen 1 install:
//! header `DataOffset`/`VolumeSize` match exactly, and all extracted entries round-trip.
//!
//! Layout: [header][file data][directory tree]. All integers little-endian.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

const DIR_ATTR: u32 = 0x0000_0010;
const DELETED_ATTR: u32 = 0x0000_8000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCompression {
    None,
    Auto,
    ZLib,
}

impl FileCompression {
    fn from_u32(v: u32) -> Self {
        match v {
            1 => FileCompression::Auto,
            2 => FileCompression::ZLib,
            _ => FileCompression::None,
        }
    }
    fn to_u32(self) -> u32 {
        match self {
            FileCompression::None => 0,
            FileCompression::Auto => 1,
            FileCompression::ZLib => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PakHeader {
    pub version: u32,
    pub product: u32,
    pub revision: u32,
    pub encryption: u32,
    pub compression: u32,
    pub data_offset: u64,
    pub root_offset: u64,
    pub volume_size: u64,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub data_offset: u64,
    pub file_attributes: u32,
    pub compression: FileCompression,
    pub data_size: u32,
    pub file_size: u32,
}

impl FileEntry {
    pub fn is_deleted(&self) -> bool {
        self.file_attributes & DELETED_ATTR != 0
    }
}

#[derive(Debug)]
enum Node {
    File(FileEntry),
    Dir { name: String, entries: Vec<Node> },
}

pub struct PakArchive {
    pub header: PakHeader,
    file: File,
    root: Node,
}

fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_name<R: Read>(r: &mut R) -> io::Result<String> {
    let len = read_u32(r)? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    let mut term = [0u8; 1];
    r.read_exact(&mut term)?; // null terminator
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn read_file_entry<R: Read>(r: &mut R, path_prefix: &str) -> io::Result<FileEntry> {
    let name = read_name(r)?;
    let data_offset = read_u64(r)?;
    let _t_created = read_u64(r)?;
    let _t_accessed = read_u64(r)?;
    let _t_modified = read_u64(r)?;
    let file_attributes = read_u32(r)?;
    let _encryption = read_u32(r)?;
    let compression = FileCompression::from_u32(read_u32(r)?);
    let data_size = read_u32(r)?;
    let file_size = read_u32(r)?;
    Ok(FileEntry {
        path: format!("{path_prefix}/{name}"),
        data_offset,
        file_attributes,
        compression,
        data_size,
        file_size,
    })
}

fn read_directory<R: Read>(r: &mut R, path_prefix: &str) -> io::Result<Node> {
    let name = read_name(r)?;
    let my_path = if name.is_empty() {
        path_prefix.to_string()
    } else {
        format!("{path_prefix}/{name}")
    };
    let _t_created = read_u64(r)?;
    let _t_accessed = read_u64(r)?;
    let _t_modified = read_u64(r)?;
    let _file_attributes = read_u32(r)?;
    let count = read_u32(r)?;
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let attributes = read_u32(r)?;
        if attributes & DIR_ATTR != 0 {
            entries.push(read_directory(r, &my_path)?);
        } else {
            entries.push(Node::File(read_file_entry(r, &my_path)?));
        }
    }
    Ok(Node::Dir {
        name: my_path,
        entries,
    })
}

fn flatten_node(node: &Node, out: &mut Vec<FileEntry>) {
    match node {
        Node::File(f) => out.push(f.clone()),
        Node::Dir { entries, .. } => {
            for e in entries {
                flatten_node(e, out);
            }
        }
    }
}

impl PakArchive {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = File::open(path)?;

        // Manually parse header so we can compute data_offset/root_offset/volume_size (u64 fields).
        let version = read_u32(&mut file)?;
        let product = read_u32(&mut file)?;
        let revision = read_u32(&mut file)?;
        let encryption = read_u32(&mut file)?;
        let compression = read_u32(&mut file)?;
        let _reserved = read_u32(&mut file)?;
        let data_offset = read_u64(&mut file)?;
        let root_offset = read_u64(&mut file)?;
        let volume_size = read_u64(&mut file)?;

        let header = PakHeader {
            version,
            product,
            revision,
            encryption,
            compression,
            data_offset,
            root_offset,
            volume_size,
        };

        file.seek(SeekFrom::Start(header.root_offset))?;
        let root = read_directory(&mut file, "")?;

        Ok(PakArchive {
            header,
            file,
            root,
        })
    }

    pub fn is_valid_g3v0(&self) -> bool {
        // "G3V0" little-endian
        self.header.product == 0x3056_3347
    }

    pub fn files(&self) -> Vec<FileEntry> {
        let mut out = Vec::new();
        flatten_node(&self.root, &mut out);
        out
    }

    /// Read and (if needed) decompress a file entry's data.
    pub fn read_file(&mut self, entry: &FileEntry) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(entry.data_offset))?;
        let mut raw = vec![0u8; entry.data_size as usize];
        self.file.read_exact(&mut raw)?;
        match entry.compression {
            FileCompression::ZLib => {
                let mut decoder = ZlibDecoder::new(&raw[..]);
                let mut out = Vec::with_capacity(entry.file_size as usize);
                decoder.read_to_end(&mut out)?;
                Ok(out)
            }
            _ => Ok(raw),
        }
    }

    pub fn extract_all<P: AsRef<Path>>(&mut self, out_dir: P) -> io::Result<usize> {
        let out_dir = out_dir.as_ref();
        let entries = self.files();
        let mut count = 0;
        for entry in &entries {
            if entry.is_deleted() {
                continue;
            }
            let data = self.read_file(entry)?;
            let rel = entry.path.trim_start_matches('/');
            let dest = out_dir.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, &data)?;
            count += 1;
        }
        Ok(count)
    }
}

/// Build a brand-new, uncompressed `.pak`/`.pXX` archive from a directory tree, preserving
/// subfolder structure. Intended for producing patch volumes (see docs/p0x-patches.md), not
/// for editing an existing archive in place.
pub fn write_archive_from_dir<P: AsRef<Path>>(src_dir: P, out_path: P) -> io::Result<()> {
    let src_dir = src_dir.as_ref();
    let mut file_list = Vec::new();
    collect_files(src_dir, src_dir, &mut file_list)?;

    let mut out = File::create(out_path.as_ref())?;

    // Header is written last-fields-known, but DataOffset is always 48 (fixed header size).
    const HEADER_SIZE: u64 = 48;
    out.write_all(&1u32.to_le_bytes())?; // version
    out.write_all(&0x3056_3347u32.to_le_bytes())?; // "G3V0"
    out.write_all(&0u32.to_le_bytes())?; // revision
    out.write_all(&0u32.to_le_bytes())?; // encryption = none
    out.write_all(&0u32.to_le_bytes())?; // compression = none (whole-volume flag)
    out.write_all(&0u32.to_le_bytes())?; // reserved
    out.write_all(&HEADER_SIZE.to_le_bytes())?; // data_offset
    let root_offset_pos = out.stream_position()?;
    out.write_all(&0u64.to_le_bytes())?; // root_offset placeholder
    let volume_size_pos = out.stream_position()?;
    out.write_all(&0u64.to_le_bytes())?; // volume_size placeholder

    let mut written = Vec::new(); // (relative_path, data_offset, size)
    for rel_path in &file_list {
        let full = src_dir.join(rel_path);
        let data = std::fs::read(&full)?;
        let offset = out.stream_position()?;
        out.write_all(&data)?;
        written.push((rel_path.clone(), offset, data.len() as u32));
    }

    let root_offset = out.stream_position()?;
    let tree = build_write_tree(&written);
    write_directory_tree(&mut out, &tree)?;
    let volume_size = out.stream_position()?;

    out.seek(SeekFrom::Start(root_offset_pos))?;
    out.write_all(&root_offset.to_le_bytes())?;
    out.seek(SeekFrom::Start(volume_size_pos))?;
    out.write_all(&volume_size.to_le_bytes())?;

    Ok(())
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push(rel);
        }
    }
    Ok(())
}

fn write_name<W: Write>(w: &mut W, name: &str) -> io::Result<()> {
    let bytes = name.as_bytes();
    w.write_all(&(bytes.len() as u32).to_le_bytes())?;
    if !bytes.is_empty() {
        w.write_all(bytes)?;
        w.write_all(&[0u8])?; // null terminator
    }
    Ok(())
}

/// In-memory shape of the directory tree being written, built from a flat list of
/// (relative_path, offset, size) by splitting each path on `/`.
enum WriteNode {
    File { name: String, offset: u64, size: u32 },
    Dir { name: String, children: Vec<WriteNode> },
}

fn build_write_tree(files: &[(String, u64, u32)]) -> Vec<WriteNode> {
    let mut root: Vec<WriteNode> = Vec::new();
    for (rel_path, offset, size) in files {
        let parts: Vec<&str> = rel_path.split('/').collect();
        insert_write_node(&mut root, &parts, *offset, *size);
    }
    root
}

fn insert_write_node(nodes: &mut Vec<WriteNode>, parts: &[&str], offset: u64, size: u32) {
    match parts {
        [] => {}
        [name] => nodes.push(WriteNode::File {
            name: (*name).to_string(),
            offset,
            size,
        }),
        [dir_name, rest @ ..] => {
            let idx = nodes
                .iter()
                .position(|n| matches!(n, WriteNode::Dir { name, .. } if name == dir_name))
                .unwrap_or_else(|| {
                    nodes.push(WriteNode::Dir {
                        name: (*dir_name).to_string(),
                        children: Vec::new(),
                    });
                    nodes.len() - 1
                });
            if let WriteNode::Dir { children, .. } = &mut nodes[idx] {
                insert_write_node(children, rest, offset, size);
            }
        }
    }
}

/// Writes the root directory record: no leading discriminator (matches how `read_directory`
/// is invoked at the root), then every child with its own discriminator attributes field.
fn write_directory_tree<W: Write>(w: &mut W, nodes: &[WriteNode]) -> io::Result<()> {
    write_name(w, "")?; // root has no name
    w.write_all(&0u64.to_le_bytes())?; // created
    w.write_all(&0u64.to_le_bytes())?; // accessed
    w.write_all(&0u64.to_le_bytes())?; // modified
    w.write_all(&DIR_ATTR.to_le_bytes())?; // directory attribute
    w.write_all(&(nodes.len() as u32).to_le_bytes())?; // count
    for node in nodes {
        write_node(w, node)?;
    }
    Ok(())
}

fn write_node<W: Write>(w: &mut W, node: &WriteNode) -> io::Result<()> {
    match node {
        WriteNode::File { name, offset, size } => {
            let file_attr = 0x0000_0020u32; // FILE_ATTRIBUTE_ARCHIVE, not a directory
            w.write_all(&file_attr.to_le_bytes())?; // discriminator (no DIR_ATTR bit set)
            write_name(w, name)?;
            w.write_all(&offset.to_le_bytes())?; // data_offset
            w.write_all(&0u64.to_le_bytes())?; // created
            w.write_all(&0u64.to_le_bytes())?; // accessed
            w.write_all(&0u64.to_le_bytes())?; // modified
            w.write_all(&file_attr.to_le_bytes())?; // file_attributes
            w.write_all(&0u32.to_le_bytes())?; // encryption = none
            w.write_all(&FileCompression::None.to_u32().to_le_bytes())?; // compression = none
            w.write_all(&size.to_le_bytes())?; // data_size
            w.write_all(&size.to_le_bytes())?; // file_size
        }
        WriteNode::Dir { name, children } => {
            w.write_all(&DIR_ATTR.to_le_bytes())?; // discriminator
            write_name(w, name)?;
            w.write_all(&0u64.to_le_bytes())?; // created
            w.write_all(&0u64.to_le_bytes())?; // accessed
            w.write_all(&0u64.to_le_bytes())?; // modified
            w.write_all(&DIR_ATTR.to_le_bytes())?; // file_attributes
            w.write_all(&(children.len() as u32).to_le_bytes())?; // count
            for child in children {
                write_node(w, child)?;
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn zlib_compress(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a source directory with nested subfolders (mirroring a real `images.pak` tree,
    /// e.g. `Level/...`, `Animation/Monster/...`), writes it to an archive, reads it back, and
    /// checks every file round-trips with its subfolder path and content intact.
    #[test]
    fn write_archive_preserves_subfolder_structure() {
        let tmp_dir = std::env::temp_dir().join(format!("risenlab_pak_write_test_{}", std::process::id()));
        let src_dir = tmp_dir.join("src");
        std::fs::create_dir_all(src_dir.join("Level")).unwrap();
        std::fs::create_dir_all(src_dir.join("Animation").join("Monster")).unwrap();

        let files: &[(&str, &[u8])] = &[
            ("root_file.txt", b"root content"),
            ("Level/Nat_Stone.txt", b"stone texture data"),
            ("Animation/Monster/Chicken.txt", b"chicken armor data"),
        ];
        for (rel, content) in files {
            let full = src_dir.join(rel);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
        }

        let archive_path = tmp_dir.join("test.pak");
        write_archive_from_dir(&src_dir, &archive_path).unwrap();

        let mut archive = PakArchive::open(&archive_path).unwrap();
        let entries = archive.files();
        assert_eq!(entries.len(), files.len());

        for (rel, content) in files {
            let expected_path = format!("/{rel}");
            let entry = entries
                .iter()
                .find(|e| e.path == expected_path)
                .unwrap_or_else(|| panic!("missing entry for {expected_path}, got {entries:?}"));
            let data = archive.read_file(entry).unwrap();
            assert_eq!(&data, content, "content mismatch for {expected_path}");
        }

        std::fs::remove_dir_all(&tmp_dir).ok();
    }
}
