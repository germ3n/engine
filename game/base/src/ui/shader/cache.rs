use naga::ShaderStage;
use std::path::PathBuf;

const VERSION: &str = "1";

pub struct Registry {
    root: PathBuf,
}

impl Registry {
    pub fn for_device(device_id: &str) -> Self {
        let root = PathBuf::from("cache/shaders").join(sanitize(device_id));
        let _ = std::fs::create_dir_all(&root);

        Self { root }
    }

    pub fn spirv(&self, source: &str, stage: ShaderStage, entry: &str) -> Result<Vec<u32>, String> {
        let stage_tag = stage_tag(stage);
        let digest = hash(&[VERSION, "spv", stage_tag, entry, source]);

        if let Some(bytes) = self.load("spv", digest) {
            if bytes.len() % 4 == 0 && !bytes.is_empty() {
                let words = bytes
                    .chunks_exact(4)
                    .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect::<Vec<_>>();

                if words.first() == Some(&0x07230203) {
                    return Ok(words);
                }
            }
        }

        let words = crate::ui::shader::spirv(source, stage, entry)?;
        let bytes = words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        self.store("spv", digest, &bytes);

        Ok(words)
    }

    pub fn hlsl(&self, source: &str) -> Result<String, String> {
        let digest = hash(&[VERSION, "hlsl", source]);

        if let Some(bytes) = self.load("hlsl", digest) {
            if let Ok(text) = String::from_utf8(bytes) {
                return Ok(text);
            }
        }

        let text = crate::ui::shader::hlsl(source)?;
        self.store("hlsl", digest, text.as_bytes());

        Ok(text)
    }

    pub fn msl(&self, source: &str) -> Result<String, String> {
        let digest = hash(&[VERSION, "msl", source]);

        if let Some(bytes) = self.load("msl", digest) {
            if let Ok(text) = String::from_utf8(bytes) {
                return Ok(text);
            }
        }

        let text = crate::ui::shader::msl(source)?;
        self.store("msl", digest, text.as_bytes());

        Ok(text)
    }

    pub fn glsl(
        &self,
        source: &str,
        stage: ShaderStage,
        entry: &str,
        version: naga::back::glsl::Version,
    ) -> Result<String, String> {
        let stage_tag = stage_tag(stage);
        let version_tag = glsl_version_tag(version);
        let digest = hash(&[VERSION, "glsl", version_tag, stage_tag, entry, source]);

        if let Some(bytes) = self.load("glsl", digest) {
            if let Ok(text) = String::from_utf8(bytes) {
                return Ok(text);
            }
        }

        let (text, _) = crate::ui::shader::glsl(source, stage, entry, version)?;
        self.store("glsl", digest, text.as_bytes());

        Ok(text)
    }

    fn load(&self, kind: &str, digest: u64) -> Option<Vec<u8>> {
        std::fs::read(self.path(kind, digest)).ok()
    }

    fn store(&self, kind: &str, digest: u64, bytes: &[u8]) {
        let path = self.path(kind, digest);
        let _ = std::fs::write(path, bytes);
    }

    fn path(&self, kind: &str, digest: u64) -> PathBuf {
        self.root.join(format!("{kind}-{digest:016x}"))
    }
}

pub fn id_from_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }

    out
}

pub fn id_from_u64(value: u64) -> String {
    format!("{value:016x}")
}

pub fn id_from_luid(low: u32, high: i32) -> String {
    format!("{high:08x}{low:08x}")
}

pub fn id_from_text(parts: &[&str]) -> String {
    format!("{:016x}", hash(parts))
}

fn sanitize(device_id: &str) -> String {
    let mut out = String::with_capacity(device_id.len());

    for ch in device_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        }
    }

    if out.is_empty() {
        out.push_str("unknown");
    }

    out
}

fn stage_tag(stage: ShaderStage) -> &'static str {
    match stage {
        ShaderStage::Vertex => "vs",
        ShaderStage::Fragment => "fs",
        ShaderStage::Compute => "cs",
        ShaderStage::Task => "task",
        ShaderStage::Mesh => "mesh",
        _ => "other",
    }
}

fn glsl_version_tag(version: naga::back::glsl::Version) -> &'static str {
    match version {
        naga::back::glsl::Version::Desktop(330) => "gl330",
        naga::back::glsl::Version::Desktop(_) => "gl",
        naga::back::glsl::Version::Embedded {
            version: 300,
            is_webgl: false,
        } => "es300",
        naga::back::glsl::Version::Embedded { .. } => "es",
    }
}

fn hash(parts: &[&str]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;

    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }

        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use naga::ShaderStage;

    #[test]
    fn registry_roundtrip_spirv() {
        let dir = std::env::temp_dir().join(format!("engine-shader-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let cache = Registry::for_device("test-device");
        let source = crate::ui::shaders::Program::Mesh.wgsl();
        let first = cache
            .spirv(&source, ShaderStage::Vertex, "vs_main")
            .unwrap();
        let second = cache
            .spirv(&source, ShaderStage::Vertex, "vs_main")
            .unwrap();
        std::env::set_current_dir(old).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(first, second);
        assert_eq!(first[0], 0x07230203);
    }
}
