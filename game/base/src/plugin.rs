use std::fs::File;
use std::path::Path;
use wincode::{SchemaRead, SchemaWrite};

const MAGIC: &[u8; 4] = b"PLUG";
const VERSION: u32 = 1;
const MAX_ENTRIES: usize = 65_536;
const MAX_RAW_SIZE: u64 = 256 * 1024 * 1024;

#[derive(SchemaWrite, SchemaRead)]
struct Entry
{
    name: String,
    offset: u64,
    compressed: u64,
    raw_size: u64,
}

#[derive(SchemaWrite, SchemaRead)]
struct Catalog
{
    version: u32,
    entries: Vec<Entry>,
}

pub struct Archive
{
    map: memmap2::Mmap,
    entries: Vec<Entry>,
}

impl Archive
{
    pub fn create(path: &Path, files: &[(&str, &[u8])]) -> Result<(), String>
    {
        let bytes = encode(files)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &bytes).map_err(io_err)?;
        let _ = std::fs::remove_file(path);
        std::fs::rename(&tmp, path).map_err(io_err)?;

        return Ok(());
    }

    pub fn open(path: &Path) -> Result<Self, String>
    {
        let file = File::open(path).map_err(io_err)?;
        let map = unsafe { memmap2::Mmap::map(&file).map_err(io_err)? };

        if map.len() < 4 || &map[..4] != MAGIC
        {
            return Err("plugin archive header is invalid".to_string());
        }

        let catalog: Catalog = wincode::deserialize(&map[4..]).map_err(|err| format!("plugin archive: {err}"))?;

        if catalog.version != VERSION
        {
            return Err(format!("plugin archive version {} is unsupported", catalog.version));
        }

        if catalog.entries.len() > MAX_ENTRIES
        {
            return Err(format!("plugin archive has {} entries", catalog.entries.len()));
        }

        let mut idx = 0;

        while idx < catalog.entries.len()
        {
            check_name(&catalog.entries[idx].name)?;
            check_sizes(&catalog.entries[idx].name, catalog.entries[idx].compressed, catalog.entries[idx].raw_size)?;

            let mut other = 0;

            while other < idx
            {
                if catalog.entries[other].name == catalog.entries[idx].name
                {
                    return Err(format!("plugin path {} is duplicated", catalog.entries[idx].name));
                }

                other += 1;
            }

            idx += 1;
        }

        let toc_len = wincode::serialized_size(&catalog).map_err(|err| format!("plugin archive: {err}"))?;
        let toc_end = 4u64.checked_add(toc_len).ok_or_else(|| "plugin archive is too large".to_string())?;

        if toc_end > map.len() as u64
        {
            return Err("plugin archive header is invalid".to_string());
        }

        check_layout(&catalog.entries, toc_end, map.len() as u64)?;

        return Ok(Self { map, entries: catalog.entries });
    }

    pub fn len(&self) -> usize
    {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool
    {
        self.entries.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> + '_
    {
        self.entries.iter().map(|entry| entry.name.as_str())
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, String>
    {
        let mut idx = 0;

        while idx < self.entries.len()
        {
            if self.entries[idx].name == name
            {
                return read_frame(&self.map, self.entries[idx].offset, self.entries[idx].compressed, self.entries[idx].raw_size);
            }

            idx += 1;
        }

        return Err(format!("plugin entry {name} was not found"));
    }
}

struct Plugin
{
    name: String,
    archive: Archive,
}

pub struct Registry
{
    plugins: Vec<Plugin>,
}

impl Registry
{
    pub fn new() -> Self
    {
        Self { plugins: Vec::new() }
    }

    pub fn mount(&mut self, path: &Path) -> Result<String, String>
    {
        let name = plugin_name(path)?;

        if self.find(&name).is_some()
        {
            return Err(format!("plugin {name} is already mounted"));
        }

        let archive = Archive::open(path)?;
        self.plugins.push(Plugin { name: name.clone(), archive });

        return Ok(name);
    }

    pub fn mount_dir(&mut self, dir: &Path) -> Result<Vec<String>, String>
    {
        let mut paths = Vec::new();
        let listing = std::fs::read_dir(dir).map_err(io_err)?;

        for entry in listing
        {
            let entry = entry.map_err(io_err)?;
            let path = entry.path();

            if is_plug(&path)
            {
                paths.push(path);
            }
        }

        paths.sort();
        let mut names = Vec::new();
        let mut idx = 0;

        while idx < paths.len()
        {
            match self.mount(&paths[idx])
            {
                Ok(name) => names.push(name),
                Err(err) =>
                {
                    let mut mounted = 0;

                    while mounted < names.len()
                    {
                        let _ = self.unmount(&names[mounted]);
                        mounted += 1;
                    }

                    return Err(err);
                }
            }

            idx += 1;
        }

        return Ok(names);
    }

    pub fn unmount(&mut self, name: &str) -> Result<(), String>
    {
        let Some(idx) = self.find(name) else
        {
            return Err(format!("plugin {name} is not mounted"));
        };

        self.plugins.remove(idx);

        return Ok(());
    }

    pub fn is_mounted(&self, name: &str) -> bool
    {
        self.find(name).is_some()
    }

    pub fn plugins(&self) -> impl Iterator<Item = &str> + '_
    {
        self.plugins.iter().map(|plugin| plugin.name.as_str())
    }

    pub fn names<'a>(&'a self, plugin: &str) -> Result<impl Iterator<Item = &'a str> + 'a, String>
    {
        let Some(idx) = self.find(plugin) else
        {
            return Err(format!("plugin {plugin} is not mounted"));
        };

        return Ok(self.plugins[idx].archive.names());
    }

    pub fn read(&self, plugin: &str, name: &str) -> Result<Vec<u8>, String>
    {
        let Some(idx) = self.find(plugin) else
        {
            return Err(format!("plugin {plugin} is not mounted"));
        };

        return self.plugins[idx].archive.read(name);
    }

    fn find(&self, name: &str) -> Option<usize>
    {
        let mut idx = 0;

        while idx < self.plugins.len()
        {
            if self.plugins[idx].name == name
            {
                return Some(idx);
            }

            idx += 1;
        }

        return None;
    }
}

fn plugin_name(path: &Path) -> Result<String, String>
{
    let stem = path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| format!("plugin path {} is invalid", path.display()))?;
    check_name(stem)?;

    return Ok(stem.to_string());
}

fn is_plug(path: &Path) -> bool
{
    match path.extension().and_then(|ext| ext.to_str())
    {
        Some(ext) => ext.eq_ignore_ascii_case("plug"),
        None => false,
    }
}

fn encode(files: &[(&str, &[u8])]) -> Result<Vec<u8>, String>
{
    if files.len() > MAX_ENTRIES
    {
        return Err(format!("plugin archive has {} entries", files.len()));
    }

    let mut idx = 0;

    while idx < files.len()
    {
        let mut other = 0;

        while other < idx
        {
            if files[other].0 == files[idx].0
            {
                return Err(format!("plugin path {} is duplicated", files[idx].0));
            }

            other += 1;
        }

        idx += 1;
    }

    let mut frames = Vec::with_capacity(files.len());
    let mut entries = Vec::with_capacity(files.len());

    for (name, bytes) in files
    {
        check_name(name)?;

        if bytes.len() as u64 > MAX_RAW_SIZE
        {
            return Err(format!("plugin entry {name} is {} bytes", bytes.len()));
        }

        let frame = zstd::bulk::compress(bytes, zstd::DEFAULT_COMPRESSION_LEVEL).map_err(io_err)?;

        if frame.len() as u64 > max_compressed(bytes.len() as u64)
        {
            return Err(format!("plugin entry {name} did not compress"));
        }

        entries.push(Entry {
            name: (*name).to_string(),
            offset: 0,
            compressed: frame.len() as u64,
            raw_size: bytes.len() as u64,
        });
        frames.push(frame);
    }

    let mut catalog = Catalog { version: VERSION, entries };
    let toc_len = wincode::serialized_size(&catalog).map_err(|err| format!("plugin archive: {err}"))?;
    let mut cursor = align8(4 + toc_len);
    idx = 0;

    while idx < catalog.entries.len()
    {
        catalog.entries[idx].offset = cursor;
        cursor = align8(cursor + catalog.entries[idx].compressed);
        idx += 1;
    }

    let again = wincode::serialized_size(&catalog).map_err(|err| format!("plugin archive: {err}"))?;

    if again != toc_len
    {
        return Err("plugin archive layout changed".to_string());
    }

    let toc = wincode::serialize(&catalog).map_err(|err| format!("plugin archive: {err}"))?;

    if toc.len() as u64 != toc_len
    {
        return Err("plugin archive layout changed".to_string());
    }

    let mut out = Vec::with_capacity(cursor as usize);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&toc);
    idx = 0;

    while idx < frames.len()
    {
        pad(&mut out, catalog.entries[idx].offset)?;
        out.extend_from_slice(&frames[idx]);
        idx += 1;
    }

    return Ok(out);
}

fn read_frame(map: &[u8], offset: u64, compressed: u64, raw_size: u64) -> Result<Vec<u8>, String>
{
    let start = usize::try_from(offset).map_err(|_| "plugin entry is too large".to_string())?;
    let len = usize::try_from(compressed).map_err(|_| "plugin entry is too large".to_string())?;
    let end = start.checked_add(len).ok_or_else(|| "plugin entry is too large".to_string())?;

    if end > map.len()
    {
        return Err("plugin entry extends past the archive".to_string());
    }

    let cap = usize::try_from(raw_size).map_err(|_| "plugin entry is too large".to_string())?;
    let raw = zstd::bulk::decompress(&map[start..end], cap).map_err(io_err)?;

    if raw.len() as u64 != raw_size
    {
        return Err("plugin entry size does not match".to_string());
    }

    return Ok(raw);
}

fn check_layout(entries: &[Entry], toc_end: u64, file_len: u64) -> Result<(), String>
{
    let data_start = align8(toc_end);
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|idx| entries[*idx].offset);
    let mut end = data_start;
    let mut idx = 0;

    while idx < order.len()
    {
        let entry = &entries[order[idx]];

        if entry.offset < data_start || entry.offset % 8 != 0 || entry.offset < end
        {
            return Err("plugin archive entries overlap".to_string());
        }

        let next = entry.offset.checked_add(entry.compressed).ok_or_else(|| "plugin entry is too large".to_string())?;

        if next > file_len
        {
            return Err("plugin entry extends past the archive".to_string());
        }

        end = align8(next);
        idx += 1;
    }

    return Ok(());
}

fn check_name(name: &str) -> Result<(), String>
{
    if name.is_empty() || name.len() > u16::MAX as usize || name.chars().any(|ch| ch.is_control())
    {
        return Err(format!("plugin path {name} is invalid"));
    }

    if name.starts_with('/') || name.as_bytes().contains(&b'\\') || name.as_bytes().contains(&b':')
    {
        return Err(format!("plugin path {name} is invalid"));
    }

    for part in name.split('/')
    {
        if part.is_empty() || part == "." || part == ".."
        {
            return Err(format!("plugin path {name} is invalid"));
        }
    }

    return Ok(());
}

fn check_sizes(name: &str, compressed: u64, raw_size: u64) -> Result<(), String>
{
    if raw_size > MAX_RAW_SIZE || compressed == 0 || compressed > max_compressed(raw_size)
    {
        return Err(format!("plugin entry {name} is {raw_size} bytes"));
    }

    return Ok(());
}

fn max_compressed(raw_size: u64) -> u64
{
    raw_size + raw_size / 256 + 64
}

fn align8(value: u64) -> u64
{
    (value + 7) & !7
}

fn pad(out: &mut Vec<u8>, offset: u64) -> Result<(), String>
{
    if (out.len() as u64) > offset
    {
        return Err("plugin archive layout overflowed".to_string());
    }

    while (out.len() as u64) < offset
    {
        out.push(0);
    }

    return Ok(());
}

fn io_err(err: std::io::Error) -> String
{
    format!("plugin archive: {err}")
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf
    {
        let dir = std::env::temp_dir().join(format!("engine-plugin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        return dir;
    }

    #[test]
    fn roundtrip_reads_one_entry_from_the_file()
    {
        let dir = scratch("roundtrip");
        let path = dir.join("mod.plug");
        let script = b"print('hi')";
        let blob = vec![7u8; 4096];
        Archive::create(&path, &[("scripts/init.lua", script), ("data/blob.bin", &blob)]).unwrap();

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
    fn create_rejects_unsafe_paths()
    {
        let dir = scratch("paths");
        let path = dir.join("mod.plug");
        assert!(Archive::create(&path, &[("../secret.lua", b"x")]).is_err());
        assert!(Archive::create(&path, &[("/etc/passwd", b"x")]).is_err());
        assert!(Archive::create(&path, &[("C:/windows", b"x")]).is_err());
        assert!(Archive::create(&path, &[("a//b.lua", b"x")]).is_err());
        assert!(Archive::create(&path, &[("a/./b.lua", b"x")]).is_err());
        assert!(Archive::create(&path, &[("a.lua", b"x"), ("a.lua", b"y")]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_rejects_an_entry_bigger_than_the_cap()
    {
        let dir = scratch("cap");
        let path = dir.join("mod.plug");
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
    fn open_rejects_a_bad_header()
    {
        let dir = scratch("header");
        let path = dir.join("mod.plug");
        std::fs::write(&path, b"NOPE").unwrap();
        assert!(Archive::open(&path).is_err());
        Archive::create(&path, &[]).unwrap();

        let archive = Archive::open(&path).unwrap();
        assert!(archive.is_empty());
        assert!(archive.read("missing.lua").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_mounts_and_unmounts()
    {
        let dir = scratch("registry");
        let hello = dir.join("hello.plug");
        let world = dir.join("world.plug");
        Archive::create(&hello, &[("scripts/init.lua", b"hello")]).unwrap();
        Archive::create(&world, &[("scripts/init.lua", b"world")]).unwrap();

        let mut registry = Registry::new();
        let names = registry.mount_dir(&dir).unwrap();
        assert_eq!(names, ["hello", "world"]);
        assert!(registry.is_mounted("hello"));
        assert_eq!(registry.read("hello", "scripts/init.lua").unwrap(), b"hello");
        assert_eq!(registry.read("world", "scripts/init.lua").unwrap(), b"world");
        assert!(registry.mount(&hello).is_err());

        registry.unmount("hello").unwrap();
        assert!(!registry.is_mounted("hello"));
        assert!(registry.read("hello", "scripts/init.lua").is_err());
        assert_eq!(registry.read("world", "scripts/init.lua").unwrap(), b"world");
        assert!(registry.unmount("hello").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
