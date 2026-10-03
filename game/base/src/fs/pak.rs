use std::fs::File;
use std::path::Path;
use wincode::{SchemaRead, SchemaWrite};

const MAGIC: &[u8; 4] = b"PAK\0";
const VERSION: u32 = 1;
const MAX_ENTRIES: usize = 65_536;
const MAX_RAW_SIZE: u64 = 1024 * 1024 * 1024;

#[derive(SchemaWrite, SchemaRead)]
struct Entry {
    name: String,
    offset: u64,
    compressed: u64,
    raw_size: u64,
}

#[derive(SchemaWrite, SchemaRead)]
struct Catalog {
    version: u32,
    entries: Vec<Entry>,
}

enum Storage {
    Map(memmap2::Mmap),
    Owned(Vec<u8>),
}

impl Storage {
    fn bytes(&self) -> &[u8] {
        match self {
            Storage::Map(map) => map,
            Storage::Owned(bytes) => bytes,
        }
    }
}

pub struct Archive {
    storage: Storage,
    entries: Vec<Entry>,
}

impl Archive {
    pub fn create(path: &Path, files: &[(&str, &[u8])]) -> Result<(), String> {
        let bytes = encode(files)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &bytes).map_err(io_err)?;
        let _ = std::fs::remove_file(path);
        std::fs::rename(&tmp, path).map_err(io_err)?;

        return Ok(());
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(io_err)?;
        let map = unsafe { memmap2::Mmap::map(&file).map_err(io_err)? };

        return from_storage(Storage::Map(map));
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        return from_storage(Storage::Owned(bytes));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().map(|entry| entry.name.as_str())
    }

    pub fn contains(&self, name: &str) -> bool {
        let mut idx = 0;

        while idx < self.entries.len() {
            if self.entries[idx].name == name {
                return true;
            }

            idx += 1;
        }

        return false;
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let mut idx = 0;

        while idx < self.entries.len() {
            if self.entries[idx].name == name {
                return read_frame(
                    self.storage.bytes(),
                    self.entries[idx].offset,
                    self.entries[idx].compressed,
                    self.entries[idx].raw_size,
                );
            }

            idx += 1;
        }

        return Err(format!("pak entry {name} was not found"));
    }

    pub fn list_prefix<'a>(&'a self, prefix: &str) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut idx = 0;

        while idx < self.entries.len() {
            let name = self.entries[idx].name.as_str();

            if prefix.is_empty() || name.starts_with(prefix) {
                out.push(name);
            }

            idx += 1;
        }

        return out;
    }
}

pub fn encode(files: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    if files.len() > MAX_ENTRIES {
        return Err(format!("pak archive has {} entries", files.len()));
    }

    let mut idx = 0;

    while idx < files.len() {
        let mut other = 0;

        while other < idx {
            if files[other].0 == files[idx].0 {
                return Err(format!("pak path {} is duplicated", files[idx].0));
            }

            other += 1;
        }

        idx += 1;
    }

    let mut frames = Vec::with_capacity(files.len());
    let mut entries = Vec::with_capacity(files.len());

    for (name, bytes) in files {
        check_name(name)?;

        if bytes.len() as u64 > MAX_RAW_SIZE {
            return Err(format!("pak entry {name} is {} bytes", bytes.len()));
        }

        let frame = zstd::bulk::compress(bytes, zstd::DEFAULT_COMPRESSION_LEVEL).map_err(io_err)?;

        if frame.len() as u64 > max_compressed(bytes.len() as u64) {
            return Err(format!("pak entry {name} did not compress"));
        }

        entries.push(Entry {
            name: (*name).to_string(),
            offset: 0,
            compressed: frame.len() as u64,
            raw_size: bytes.len() as u64,
        });
        frames.push(frame);
    }

    let mut catalog = Catalog {
        version: VERSION,
        entries,
    };
    let toc_len =
        wincode::serialized_size(&catalog).map_err(|err| format!("pak archive: {err}"))?;
    let mut cursor = align8(4 + toc_len);
    idx = 0;

    while idx < catalog.entries.len() {
        catalog.entries[idx].offset = cursor;
        cursor = align8(cursor + catalog.entries[idx].compressed);
        idx += 1;
    }

    let again = wincode::serialized_size(&catalog).map_err(|err| format!("pak archive: {err}"))?;

    if again != toc_len {
        return Err("pak archive layout changed".to_string());
    }

    let toc = wincode::serialize(&catalog).map_err(|err| format!("pak archive: {err}"))?;

    if toc.len() as u64 != toc_len {
        return Err("pak archive layout changed".to_string());
    }

    let mut out = Vec::with_capacity(cursor as usize);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&toc);
    idx = 0;

    while idx < frames.len() {
        pad(&mut out, catalog.entries[idx].offset)?;
        out.extend_from_slice(&frames[idx]);
        idx += 1;
    }

    return Ok(out);
}

fn from_storage(storage: Storage) -> Result<Archive, String> {
    let map = storage.bytes();

    if map.len() < 4 || &map[..4] != MAGIC {
        return Err("pak archive header is invalid".to_string());
    }

    let catalog: Catalog =
        wincode::deserialize(&map[4..]).map_err(|err| format!("pak archive: {err}"))?;

    if catalog.version != VERSION {
        return Err(format!(
            "pak archive version {} is unsupported",
            catalog.version
        ));
    }

    if catalog.entries.len() > MAX_ENTRIES {
        return Err(format!("pak archive has {} entries", catalog.entries.len()));
    }

    let mut idx = 0;

    while idx < catalog.entries.len() {
        check_name(&catalog.entries[idx].name)?;
        check_sizes(
            &catalog.entries[idx].name,
            catalog.entries[idx].compressed,
            catalog.entries[idx].raw_size,
        )?;

        let mut other = 0;

        while other < idx {
            if catalog.entries[other].name == catalog.entries[idx].name {
                return Err(format!(
                    "pak path {} is duplicated",
                    catalog.entries[idx].name
                ));
            }

            other += 1;
        }

        idx += 1;
    }

    let toc_len =
        wincode::serialized_size(&catalog).map_err(|err| format!("pak archive: {err}"))?;
    let toc_end = 4u64
        .checked_add(toc_len)
        .ok_or_else(|| "pak archive is too large".to_string())?;

    if toc_end > map.len() as u64 {
        return Err("pak archive header is invalid".to_string());
    }

    check_layout(&catalog.entries, toc_end, map.len() as u64)?;

    return Ok(Archive {
        storage,
        entries: catalog.entries,
    });
}

fn read_frame(map: &[u8], offset: u64, compressed: u64, raw_size: u64) -> Result<Vec<u8>, String> {
    let start = usize::try_from(offset).map_err(|_| "pak entry is too large".to_string())?;
    let len = usize::try_from(compressed).map_err(|_| "pak entry is too large".to_string())?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| "pak entry is too large".to_string())?;

    if end > map.len() {
        return Err("pak entry extends past the archive".to_string());
    }

    let cap = usize::try_from(raw_size).map_err(|_| "pak entry is too large".to_string())?;
    let raw = zstd::bulk::decompress(&map[start..end], cap).map_err(io_err)?;

    if raw.len() as u64 != raw_size {
        return Err("pak entry size does not match".to_string());
    }

    return Ok(raw);
}

fn check_layout(entries: &[Entry], toc_end: u64, file_len: u64) -> Result<(), String> {
    let data_start = align8(toc_end);
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|idx| entries[*idx].offset);
    let mut end = data_start;
    let mut idx = 0;

    while idx < order.len() {
        let entry = &entries[order[idx]];

        if entry.offset < data_start || entry.offset % 8 != 0 || entry.offset < end {
            return Err("pak archive entries overlap".to_string());
        }

        let next = entry
            .offset
            .checked_add(entry.compressed)
            .ok_or_else(|| "pak entry is too large".to_string())?;

        if next > file_len {
            return Err("pak entry extends past the archive".to_string());
        }

        end = align8(next);
        idx += 1;
    }

    return Ok(());
}

pub(crate) fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > u16::MAX as usize || name.chars().any(|ch| ch.is_control()) {
        return Err(format!("pak path {name} is invalid"));
    }

    if name.starts_with('/') || name.as_bytes().contains(&b'\\') || name.as_bytes().contains(&b':')
    {
        return Err(format!("pak path {name} is invalid"));
    }

    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("pak path {name} is invalid"));
        }
    }

    return Ok(());
}

fn check_sizes(name: &str, compressed: u64, raw_size: u64) -> Result<(), String> {
    if raw_size > MAX_RAW_SIZE || compressed == 0 || compressed > max_compressed(raw_size) {
        return Err(format!("pak entry {name} is {raw_size} bytes"));
    }

    return Ok(());
}

fn max_compressed(raw_size: u64) -> u64 {
    raw_size + raw_size / 256 + 64
}

fn align8(value: u64) -> u64 {
    (value + 7) & !7
}

fn pad(out: &mut Vec<u8>, offset: u64) -> Result<(), String> {
    if (out.len() as u64) > offset {
        return Err("pak archive layout overflowed".to_string());
    }

    while (out.len() as u64) < offset {
        out.push(0);
    }

    return Ok(());
}

fn io_err(err: std::io::Error) -> String {
    format!("pak archive: {err}")
}

pub fn is_pak(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => ext.eq_ignore_ascii_case("pak"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("engine-pak-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        return dir;
    }

    #[test]
    fn roundtrip_reads_one_entry_from_the_file() {
        let dir = scratch("roundtrip");
        let path = dir.join("mod.pak");
        let script = b"print('hi')";
        let blob = vec![7u8; 4096];
        Archive::create(
            &path,
            &[("scripts/init.lua", script), ("data/blob.bin", &blob)],
        )
        .unwrap();

        let archive = Archive::open(&path).unwrap();
        let names: Vec<&str> = archive.names().collect();
        assert_eq!(names, ["scripts/init.lua", "data/blob.bin"]);
        assert_eq!(archive.read("scripts/init.lua").unwrap(), script);

        let second = archive.entries[1].offset as usize;
        drop(archive);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[second] ^= 0xff;
        std::fs::write(&path, &bytes).unwrap();

        let archive = Archive::open(&path).unwrap();
        assert_eq!(archive.read("scripts/init.lua").unwrap(), script);
        assert!(archive.read("data/blob.bin").is_err());
        assert!(std::fs::metadata(&path).unwrap().len() < blob.len() as u64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_rejects_unsafe_paths() {
        let dir = scratch("paths");
        let path = dir.join("mod.pak");
        assert!(Archive::create(&path, &[("../secret.lua", b"x")]).is_err());
        assert!(Archive::create(&path, &[("/etc/passwd", b"x")]).is_err());
        assert!(Archive::create(&path, &[("C:/windows", b"x")]).is_err());
        assert!(Archive::create(&path, &[("a//b.lua", b"x")]).is_err());
        assert!(Archive::create(&path, &[("a/./b.lua", b"x")]).is_err());
        assert!(Archive::create(&path, &[("a.lua", b"x"), ("a.lua", b"y")]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_rejects_an_entry_bigger_than_the_cap() {
        let dir = scratch("cap");
        let path = dir.join("mod.pak");
        let catalog = Catalog {
            version: VERSION,
            entries: vec![Entry {
                name: "a.lua".to_string(),
                offset: 0,
                compressed: 1,
                raw_size: MAX_RAW_SIZE + 1,
            }],
        };
        let toc = wincode::serialize(&catalog).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&toc);
        std::fs::write(&path, &bytes).unwrap();
        assert!(Archive::open(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_rejects_a_bad_header() {
        let dir = scratch("header");
        let path = dir.join("mod.pak");
        std::fs::write(&path, b"NOPE").unwrap();
        assert!(Archive::open(&path).is_err());
        Archive::create(&path, &[]).unwrap();

        let archive = Archive::open(&path).unwrap();
        assert!(archive.is_empty());
        assert!(archive.read("missing.lua").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn base_pak_contains_the_model() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("base.pak");
        let archive = Archive::open(&path).unwrap();
        assert!(archive.contains("models/qwen.gguf"));
    }

    #[test]
    fn from_bytes_matches_open() {
        let files = [("lua/menu/menu.luac", &b"\x1bLJtest"[..])];
        let bytes = encode(&files).unwrap();
        let archive = Archive::from_bytes(bytes).unwrap();
        assert_eq!(archive.read("lua/menu/menu.luac").unwrap(), b"\x1bLJtest");
    }
}
