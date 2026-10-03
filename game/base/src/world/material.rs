use super::surface::{
    CpuImage, CpuMaterial, CubeImage, MaterialGpu, PixelFormat, FLAG_ALPHA, FLAG_BASE2,
    FLAG_BLENDMOD, FLAG_BUMP, FLAG_BUMP2, FLAG_DETAIL, FLAG_ENV, FLAG_MASK, FLAG_PHONG, FLAG_SELF,
    FLAG_SSBUMP, MODE_ADD, MODE_BLEND, MODE_LIGHT, MODE_MODULATE, MODE_UNLIT, MODE_WATER,
};
use source_vmt::Vmt;
use source_vpk::Vpk;
use source_vtf::{ImageFormat, Vtf};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct FileSource {
    pak: HashMap<String, Vec<u8>>,
    vpks: Vec<Vpk>,
}

pub struct MaterialBank {
    source: FileSource,
    materials: Vec<CpuMaterial>,
    names: HashMap<String, u16>,
}

impl FileSource {
    pub fn game() -> Self {
        Self {
            pak: HashMap::new(),
            vpks: mount_vpks(),
        }
    }

    pub fn with_pak(pak: HashMap<String, Vec<u8>>) -> Self {
        Self {
            pak,
            vpks: mount_vpks(),
        }
    }

    pub fn pull(&mut self, bsp: &vbsp::Bsp, path: &str) {
        let key = normalize_path(path);

        if self.pak.contains_key(&key) {
            return;
        }

        if let Ok(Some(bytes)) = bsp.pack.get(&key) {
            self.pak.insert(key, bytes);
        }
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let key = normalize_path(path);

        if let Some(bytes) = self.pak.get(&key) {
            return Some(bytes.clone());
        }

        let mut idx = 0;

        while idx < self.vpks.len() {
            if let Ok(bytes) = self.vpks[idx].read(&key) {
                return Some(bytes);
            }

            idx += 1;
        }

        loose_file(&key)
    }
}

impl MaterialBank {
    pub fn new(source: FileSource) -> Self {
        Self {
            source,
            materials: Vec::new(),
            names: HashMap::new(),
        }
    }

    pub fn materials(&self) -> &[CpuMaterial] {
        &self.materials
    }

    pub fn into_materials(self) -> Vec<CpuMaterial> {
        self.materials
    }

    pub fn ordered_names(&self) -> Vec<String> {
        let mut names = vec![String::new(); self.materials.len()];

        for (name, id) in &self.names {
            if let Some(slot) = names.get_mut(*id as usize) {
                if slot.is_empty() {
                    *slot = name.clone();
                }
            }
        }

        names
    }

    pub fn load(&mut self, name: &str) -> u16 {
        self.load_packed(None, name)
    }

    pub fn load_packed(&mut self, bsp: Option<&vbsp::Bsp>, name: &str) -> u16 {
        let key = material_key(name);

        if key.is_empty() {
            return u16::MAX;
        }

        if let Some(id) = self.names.get(&key) {
            return *id;
        }

        if let Some(bsp) = bsp {
            self.prepare(bsp, &key, 0);
        }

        let material = resolve_material(&self.source, &key, 0).unwrap_or_else(CpuMaterial::missing);
        let id = self.materials.len() as u16;
        self.names.insert(key, id);
        self.materials.push(material);

        id
    }

    fn prepare(&mut self, bsp: &vbsp::Bsp, name: &str, depth: u32) {
        if depth > 8 {
            return;
        }

        self.source.pull(bsp, &vmt_path(name));
        let Some(text) = read_text(&self.source, &vmt_path(name)) else {
            return;
        };
        let Ok(doc) = Vmt::from_str(&text) else {
            return;
        };

        if doc.shader == "patch" {
            if let Some(include) = doc
                .get_string("include")
                .or_else(|| doc.get_string("$include"))
            {
                self.prepare(bsp, &include, depth + 1);
            }
        }

        for key in [
            "basetexture",
            "basetexture2",
            "bumpmap",
            "bumpmap2",
            "detail",
            "blendmodulatetexture",
            "envmapmask",
        ] {
            if let Some(texture) = doc.get_string(key) {
                self.source.pull(bsp, &vtf_path(&texture));
            }
        }
    }

    pub fn read_cube(&self, paths: [&str; 6]) -> Option<CubeImage> {
        let mut faces = Vec::with_capacity(6);
        let mut idx = 0;

        while idx < 6 {
            let bytes = self.source.read(paths[idx])?;
            faces.push(image_from_vtf(&bytes).unwrap_or_else(CpuImage::checker));
            idx += 1;
        }

        Some(CubeImage {
            faces: [
                faces.remove(0),
                faces.remove(0),
                faces.remove(0),
                faces.remove(0),
                faces.remove(0),
                faces.remove(0),
            ],
        })
    }

    pub fn read_vtf_cube(&self, path: &str) -> Option<CubeImage> {
        let bytes = self.source.read(path)?;
        let vtf = Vtf::from_bytes(&bytes).ok()?;
        let mut faces = Vec::with_capacity(6);
        let mut idx = 0;

        while idx < 6 {
            faces.push(image_from_vtf_face(&vtf, idx as u8)?);
            idx += 1;
        }

        Some(CubeImage {
            faces: [
                faces.remove(0),
                faces.remove(0),
                faces.remove(0),
                faces.remove(0),
                faces.remove(0),
                faces.remove(0),
            ],
        })
    }
}

pub fn pak_from_bsp(bsp: &vbsp::Bsp, names: &[String]) -> HashMap<String, Vec<u8>> {
    let mut pak = HashMap::new();
    let mut idx = 0;

    while idx < names.len() {
        remember_pak(bsp, &mut pak, &names[idx]);
        idx += 1;
    }

    pak
}

pub fn remember_pak(bsp: &vbsp::Bsp, pak: &mut HashMap<String, Vec<u8>>, path: &str) {
    let key = normalize_path(path);

    if pak.contains_key(&key) {
        return;
    }

    if let Ok(Some(bytes)) = bsp.pack.get(&key) {
        pak.insert(key, bytes);
    }
}

fn resolve_material(source: &FileSource, name: &str, depth: u32) -> Option<CpuMaterial> {
    if depth > 8 {
        return None;
    }

    let text = read_text(source, &vmt_path(name))?;
    let doc = Vmt::from_str(&text).ok()?;

    if doc.shader == "patch" {
        let include = doc
            .get_string("include")
            .or_else(|| doc.get_string("$include"))?;
        let mut base = resolve_vmt(source, &include, depth + 1)?;
        base.apply_patch(&doc);

        return Some(material_from_vmt(source, &base));
    }

    Some(material_from_vmt(source, &doc))
}

fn resolve_vmt(source: &FileSource, name: &str, depth: u32) -> Option<Vmt> {
    if depth > 8 {
        return None;
    }

    let text = read_text(source, &vmt_path(name))?;
    let doc = Vmt::from_str(&text).ok()?;

    if doc.shader == "patch" {
        let include = doc
            .get_string("include")
            .or_else(|| doc.get_string("$include"))?;
        let mut base = resolve_vmt(source, &include, depth + 1)?;
        base.apply_patch(&doc);

        return Some(base);
    }

    Some(doc)
}

fn material_from_vmt(source: &FileSource, doc: &Vmt) -> CpuMaterial {
    let shader = doc.shader.as_str();
    let mut gpu = MaterialGpu::shaded();
    let mut flags = 0u32;
    let base_name = doc.get_string("basetexture").unwrap_or_default();
    let base = load_texture(source, &base_name);
    let width = base.width.max(1);
    let height = base.height.max(1);
    let base2 = optional_texture(source, doc, "basetexture2");
    let bump = optional_texture(source, doc, "bumpmap");
    let bump2 = optional_texture(source, doc, "bumpmap2");
    let detail = optional_texture(source, doc, "detail");
    let blend = optional_texture(source, doc, "blendmodulatetexture");
    let mask = optional_texture(source, doc, "envmapmask");

    if base2.is_some() || shader == "worldvertextransition" {
        flags |= FLAG_BASE2;
        gpu.params[0] = MODE_BLEND;
    }

    if bump.is_some() {
        flags |= FLAG_BUMP;
    }

    if bump2.is_some() {
        flags |= FLAG_BUMP2;
    }

    if detail.is_some() {
        flags |= FLAG_DETAIL;
    }

    if blend.is_some() {
        flags |= FLAG_BLENDMOD;
    }

    if mask.is_some() {
        flags |= FLAG_MASK;
    }

    if flag(doc, "ssbump") {
        flags |= FLAG_SSBUMP;
    }

    if flag(doc, "alphatest") {
        flags |= FLAG_ALPHA;
    }

    if flag(doc, "selfillum") {
        flags |= FLAG_SELF;
    }

    if flag(doc, "phong") {
        flags |= FLAG_PHONG;
    }

    if doc.get_string("envmap").is_some() {
        flags |= FLAG_ENV;
    }

    gpu.params[0] = mode_of(shader, &gpu, doc);
    gpu.params[1] = doc.get_f32("alphatestreference").unwrap_or(0.5);
    gpu.params[2] = doc.get_f32("detailblendmode").unwrap_or(0.0);
    gpu.params[3] = doc.get_f32("alpha").unwrap_or(1.0);
    gpu.tint = color_tint(doc.get_color("color"));
    gpu.detail[0] = doc.get_f32("detailscale").unwrap_or(1.0);
    gpu.detail[1] = gpu.detail[0];

    if let Some(scale) = doc.get_string("detailscale") {
        let parts: Vec<f32> = scale
            .split_whitespace()
            .filter_map(|part| part.parse().ok())
            .collect();

        if parts.len() >= 2 {
            gpu.detail[0] = parts[0];
            gpu.detail[1] = parts[1];
        }
    }

    gpu.detail[2] = doc.get_f32("detailblendfactor").unwrap_or(1.0);
    gpu.detail[3] = f32::from_bits(flags);
    gpu.env = [
        doc.get_color("envmaptint").map(|c| c[0]).unwrap_or(1.0),
        doc.get_color("envmaptint").map(|c| c[1]).unwrap_or(1.0),
        doc.get_color("envmaptint").map(|c| c[2]).unwrap_or(1.0),
        doc.get_f32("phongboost").unwrap_or(1.0),
    ];
    gpu.extra = [
        doc.get_f32("phongexponent").unwrap_or(16.0),
        scroll_component(doc, 0),
        scroll_component(doc, 1),
        if flags & FLAG_SELF != 0 { 1.0 } else { 0.0 },
    ];
    let fog = doc.get_color("fogcolor").unwrap_or([0.15, 0.35, 0.4]);
    gpu.fog = [fog[0], fog[1], fog[2], 2.0];

    CpuMaterial {
        gpu,
        width,
        height,
        base,
        base2: base2.unwrap_or_else(CpuImage::white),
        bump: bump.unwrap_or_else(CpuImage::flat_normal),
        bump2: bump2.unwrap_or_else(CpuImage::flat_normal),
        detail: detail.unwrap_or_else(CpuImage::white),
        blend: blend.unwrap_or_else(CpuImage::white),
        mask: mask.unwrap_or_else(CpuImage::white),
    }
}

fn mode_of(shader: &str, gpu: &MaterialGpu, doc: &Vmt) -> f32 {
    if shader == "water" {
        return MODE_WATER;
    }

    if shader == "unlitgeneric" {
        if flag(doc, "additive") {
            return MODE_ADD;
        }

        return MODE_UNLIT;
    }

    if shader.contains("decal") && shader.contains("modulate") {
        return MODE_MODULATE;
    }

    if shader == "worldvertextransition" || gpu.params[0] == MODE_BLEND {
        return MODE_BLEND;
    }

    if flag(doc, "additive") {
        return MODE_ADD;
    }

    MODE_LIGHT
}

fn flag(doc: &Vmt, key: &str) -> bool {
    matches!(
        doc.get_string(key).as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

fn color_tint(color: Option<[f32; 3]>) -> [f32; 4] {
    let [r, g, b] = color.unwrap_or([1.0, 1.0, 1.0]);

    [r, g, b, 1.0]
}

fn scroll_component(doc: &Vmt, index: usize) -> f32 {
    let Some(text) = doc.get_string("bumptransform") else {
        return 0.0;
    };
    let parts: Vec<f32> = text
        .split_whitespace()
        .filter_map(|part| part.parse().ok())
        .collect();

    parts.get(index).copied().unwrap_or(0.0)
}

fn optional_texture(source: &FileSource, doc: &Vmt, key: &str) -> Option<CpuImage> {
    let name = doc.get_string(key)?;

    if name.is_empty() {
        return None;
    }

    Some(load_texture(source, &name))
}

fn load_texture(source: &FileSource, name: &str) -> CpuImage {
    let path = vtf_path(name);
    let Some(bytes) = source.read(&path) else {
        return CpuImage::checker();
    };

    image_from_vtf(&bytes).unwrap_or_else(CpuImage::checker)
}

pub fn read_texture(name: &str) -> Option<CpuImage> {
    let source = FileSource::game();
    let bytes = source.read(&vtf_path(name))?;

    image_from_vtf(&bytes)
}

pub fn image_from_vtf(bytes: &[u8]) -> Option<CpuImage> {
    let vtf = Vtf::from_bytes(bytes).ok()?;

    image_from_vtf_face(&vtf, 0)
}

fn image_from_vtf_face(vtf: &Vtf, face: u8) -> Option<CpuImage> {
    let base = vtf.subresource(0, 0, face, 0).ok()?;
    let mut image = image_from_raw(base.format(), base.width(), base.height(), base.data());
    let count = vtf.mip_count();
    let mut level = 1u8;

    while level < count {
        let Some(sub) = vtf.subresource(level, 0, face, 0).ok() else {
            break;
        };
        let decoded = image_from_raw(sub.format(), sub.width(), sub.height(), sub.data());
        let expect_w = (image.width >> level).max(1);
        let expect_h = (image.height >> level).max(1);

        if decoded.format != image.format || decoded.width != expect_w || decoded.height != expect_h
        {
            break;
        }

        image.mips.push(decoded.bytes);
        level += 1;
    }

    Some(image)
}

pub fn image_from_raw(format: ImageFormat, width: u16, height: u16, data: &[u8]) -> CpuImage {
    let width = u32::from(width);
    let height = u32::from(height);

    if let Some(pixel) = compressed_format(format) {
        if width % 4 == 0 && height % 4 == 0 && !data.is_empty() {
            return CpuImage {
                width,
                height,
                format: pixel,
                bytes: data.to_vec(),
                mips: Vec::new(),
            };
        }
    }

    if format == ImageFormat::Dxt3 {
        if let Some(bytes) = decode_dxt3(width, height, data) {
            return CpuImage {
                width,
                height,
                format: PixelFormat::Rgba8,
                bytes,
                mips: Vec::new(),
            };
        }
    }

    if let Some(bytes) = decode_uncompressed(format, width, height, data) {
        return CpuImage {
            width,
            height,
            format: PixelFormat::Rgba8,
            bytes,
            mips: Vec::new(),
        };
    }

    CpuImage::checker()
}

fn compressed_format(format: ImageFormat) -> Option<PixelFormat> {
    match format {
        ImageFormat::Dxt1 | ImageFormat::Dxt1OneBitAlpha => Some(PixelFormat::Bc1),
        ImageFormat::Dxt5 => Some(PixelFormat::Bc3),
        ImageFormat::Ati2n => Some(PixelFormat::Bc5),
        ImageFormat::Ati1n => Some(PixelFormat::Bc1),
        _ => None,
    }
}

fn decode_uncompressed(
    format: ImageFormat,
    width: u32,
    height: u32,
    data: &[u8],
) -> Option<Vec<u8>> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    let mut out = vec![0u8; pixels * 4];
    let mut idx = 0;

    match format {
        ImageFormat::Rgba8888 | ImageFormat::Rgbx8888 => {
            if data.len() < pixels * 4 {
                return None;
            }

            out.copy_from_slice(&data[..pixels * 4]);
        }
        ImageFormat::Bgra8888 | ImageFormat::Bgrx8888 => {
            if data.len() < pixels * 4 {
                return None;
            }

            while idx < pixels {
                let src = idx * 4;
                out[src] = data[src + 2];
                out[src + 1] = data[src + 1];
                out[src + 2] = data[src];
                out[src + 3] = if format == ImageFormat::Bgrx8888 {
                    255
                } else {
                    data[src + 3]
                };
                idx += 1;
            }
        }
        ImageFormat::Abgr8888 => {
            if data.len() < pixels * 4 {
                return None;
            }

            while idx < pixels {
                let src = idx * 4;
                out[src] = data[src + 3];
                out[src + 1] = data[src + 2];
                out[src + 2] = data[src + 1];
                out[src + 3] = data[src];
                idx += 1;
            }
        }
        ImageFormat::Argb8888 => {
            if data.len() < pixels * 4 {
                return None;
            }

            while idx < pixels {
                let src = idx * 4;
                out[src] = data[src + 1];
                out[src + 1] = data[src + 2];
                out[src + 2] = data[src + 3];
                out[src + 3] = data[src];
                idx += 1;
            }
        }
        ImageFormat::Rgb888 | ImageFormat::Rgb888Bluescreen => {
            if data.len() < pixels * 3 {
                return None;
            }

            while idx < pixels {
                out[idx * 4] = data[idx * 3];
                out[idx * 4 + 1] = data[idx * 3 + 1];
                out[idx * 4 + 2] = data[idx * 3 + 2];
                out[idx * 4 + 3] = 255;
                idx += 1;
            }
        }
        ImageFormat::Bgr888 | ImageFormat::Bgr888Bluescreen => {
            if data.len() < pixels * 3 {
                return None;
            }

            while idx < pixels {
                out[idx * 4] = data[idx * 3 + 2];
                out[idx * 4 + 1] = data[idx * 3 + 1];
                out[idx * 4 + 2] = data[idx * 3];
                out[idx * 4 + 3] = 255;
                idx += 1;
            }
        }
        ImageFormat::I8 | ImageFormat::A8 => {
            if data.len() < pixels {
                return None;
            }

            while idx < pixels {
                let value = data[idx];
                out[idx * 4] = value;
                out[idx * 4 + 1] = value;
                out[idx * 4 + 2] = value;
                out[idx * 4 + 3] = if format == ImageFormat::A8 {
                    value
                } else {
                    255
                };
                idx += 1;
            }
        }
        ImageFormat::Ia88 | ImageFormat::Uv88 => {
            if data.len() < pixels * 2 {
                return None;
            }

            while idx < pixels {
                out[idx * 4] = data[idx * 2];
                out[idx * 4 + 1] = data[idx * 2 + 1];
                out[idx * 4 + 2] = 255;
                out[idx * 4 + 3] = 255;
                idx += 1;
            }
        }
        ImageFormat::Rgba16161616F => {
            if data.len() < pixels * 8 {
                return None;
            }

            while idx < pixels {
                let src = idx * 8;
                out[idx * 4] = half_byte(u16::from_le_bytes([data[src], data[src + 1]]));
                out[idx * 4 + 1] = half_byte(u16::from_le_bytes([data[src + 2], data[src + 3]]));
                out[idx * 4 + 2] = half_byte(u16::from_le_bytes([data[src + 4], data[src + 5]]));
                out[idx * 4 + 3] = half_byte(u16::from_le_bytes([data[src + 6], data[src + 7]]));
                idx += 1;
            }
        }
        _ => return None,
    }

    Some(out)
}

fn half_byte(bits: u16) -> u8 {
    let value = half_to_f32(bits).clamp(0.0, 1.0);

    (value * 255.0) as u8
}

fn half_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let frac = (bits & 0x3ff) as u32;

    let packed = if exp == 0 {
        if frac == 0 {
            sign << 31
        } else {
            let mut mantissa = frac;
            let mut exponent = 127 - 15 + 1;

            while mantissa & 0x400 == 0 {
                mantissa <<= 1;
                exponent -= 1;
            }

            mantissa &= 0x3ff;
            (sign << 31) | (exponent << 23) | (mantissa << 13)
        }
    } else if exp == 31 {
        (sign << 31) | (0xff << 23) | (frac << 13)
    } else {
        (sign << 31) | ((exp + (127 - 15)) << 23) | (frac << 13)
    };

    f32::from_bits(packed)
}

fn decode_dxt3(width: u32, height: u32, data: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }

    let blocks_x = ((width + 3) / 4) as usize;
    let blocks_y = ((height + 3) / 4) as usize;
    let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
    let mut block = 0;

    while block < blocks_x * blocks_y {
        let src = block * 16;

        if src + 16 > data.len() {
            return None;
        }

        let bx = block % blocks_x;
        let by = block / blocks_x;
        let color = decode_dxt1_block(&data[src + 8..src + 16]);
        let mut py = 0;

        while py < 4 {
            let alpha_bits = u16::from_le_bytes([data[src + py * 2], data[src + py * 2 + 1]]);
            let mut px = 0;

            while px < 4 {
                let x = bx * 4 + px;
                let y = by * 4 + py;

                if x < width as usize && y < height as usize {
                    let alpha = ((alpha_bits >> (px * 4)) & 0xf) as u8;
                    let pixel = color[py * 4 + px];
                    let dst = (y * width as usize + x) * 4;
                    out[dst] = pixel[0];
                    out[dst + 1] = pixel[1];
                    out[dst + 2] = pixel[2];
                    out[dst + 3] = alpha * 17;
                }

                px += 1;
            }

            py += 1;
        }

        block += 1;
    }

    Some(out)
}

pub fn image_rgba(image: &CpuImage) -> Vec<u8> {
    let pixels = (image.width as usize).saturating_mul(image.height as usize);

    if pixels == 0 {
        return vec![255, 255, 255, 255];
    }

    match image.format {
        PixelFormat::Rgba8 | PixelFormat::Rgba16f => {
            if image.bytes.len() >= pixels * 4 {
                return image.bytes[..pixels * 4].to_vec();
            }
        }
        PixelFormat::Bc1 => {
            if let Some(bytes) = decode_bc1(image.width, image.height, &image.bytes) {
                return bytes;
            }
        }
        PixelFormat::Bc2 => {
            if let Some(bytes) = decode_dxt3(image.width, image.height, &image.bytes) {
                return bytes;
            }
        }
        PixelFormat::Bc3 => {
            if let Some(bytes) = decode_bc3(image.width, image.height, &image.bytes) {
                return bytes;
            }
        }
        PixelFormat::Bc5 | PixelFormat::Bc7 => {}
    }

    let mut out = Vec::with_capacity(pixels * 4);
    let mut idx = 0;

    while idx < pixels {
        out.extend_from_slice(&[255, 255, 255, 255]);
        idx += 1;
    }

    out
}

fn decode_bc1(width: u32, height: u32, data: &[u8]) -> Option<Vec<u8>> {
    decode_blocks(width, height, data, 8, false)
}

fn decode_bc3(width: u32, height: u32, data: &[u8]) -> Option<Vec<u8>> {
    decode_blocks(width, height, data, 16, true)
}

fn decode_blocks(
    width: u32,
    height: u32,
    data: &[u8],
    block_bytes: usize,
    dxt5: bool,
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }

    let blocks_x = ((width + 3) / 4) as usize;
    let blocks_y = ((height + 3) / 4) as usize;
    let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
    let mut block = 0;

    while block < blocks_x * blocks_y {
        let src = block * block_bytes;

        if src + block_bytes > data.len() {
            return None;
        }

        let color_at = if dxt5 { src + 8 } else { src };
        let color = decode_dxt1_block(&data[color_at..color_at + 8]);
        let alpha = if dxt5 {
            dxt5_alpha(&data[src..src + 8])
        } else {
            [255u8; 16]
        };
        let bx = block % blocks_x;
        let by = block / blocks_x;
        let mut py = 0;

        while py < 4 {
            let mut px = 0;

            while px < 4 {
                let x = bx * 4 + px;
                let y = by * 4 + py;

                if x < width as usize && y < height as usize {
                    let pixel = color[py * 4 + px];
                    let dst = (y * width as usize + x) * 4;
                    out[dst] = pixel[0];
                    out[dst + 1] = pixel[1];
                    out[dst + 2] = pixel[2];
                    out[dst + 3] = if dxt5 { alpha[py * 4 + px] } else { pixel[3] };
                }

                px += 1;
            }

            py += 1;
        }

        block += 1;
    }

    Some(out)
}

fn dxt5_alpha(data: &[u8]) -> [u8; 16] {
    let a0 = data[0];
    let a1 = data[1];
    let mut bits = 0u64;
    let mut idx = 0;

    while idx < 6 {
        bits |= (data[2 + idx] as u64) << (8 * idx);
        idx += 1;
    }

    let mut table = [a0, a1, 0, 0, 0, 0, 0, 0];

    if a0 > a1 {
        table[2] = ((6 * u16::from(a0) + u16::from(a1)) / 7) as u8;
        table[3] = ((5 * u16::from(a0) + 2 * u16::from(a1)) / 7) as u8;
        table[4] = ((4 * u16::from(a0) + 3 * u16::from(a1)) / 7) as u8;
        table[5] = ((3 * u16::from(a0) + 4 * u16::from(a1)) / 7) as u8;
        table[6] = ((2 * u16::from(a0) + 5 * u16::from(a1)) / 7) as u8;
        table[7] = ((u16::from(a0) + 6 * u16::from(a1)) / 7) as u8;
    } else {
        table[2] = ((4 * u16::from(a0) + u16::from(a1)) / 5) as u8;
        table[3] = ((3 * u16::from(a0) + 2 * u16::from(a1)) / 5) as u8;
        table[4] = ((2 * u16::from(a0) + 3 * u16::from(a1)) / 5) as u8;
        table[5] = ((u16::from(a0) + 4 * u16::from(a1)) / 5) as u8;
        table[6] = 0;
        table[7] = 255;
    }

    let mut out = [0u8; 16];
    idx = 0;

    while idx < 16 {
        let code = ((bits >> (idx * 3)) & 7) as usize;
        out[idx] = table[code];
        idx += 1;
    }

    out
}

fn decode_dxt1_block(data: &[u8]) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([data[0], data[1]]);
    let c1 = u16::from_le_bytes([data[2], data[3]]);
    let bits = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let color0 = rgb565(c0);
    let color1 = rgb565(c1);
    let mut colors = [color0, color1, [0, 0, 0, 255], [0, 0, 0, 255]];

    if c0 > c1 {
        colors[2] = lerp_color(color0, color1, 1, 3);
        colors[3] = lerp_color(color0, color1, 2, 3);
    } else {
        colors[2] = lerp_color(color0, color1, 1, 2);
        colors[3][3] = 0;
    }

    let mut pixels = [[0u8; 4]; 16];
    let mut idx = 0;

    while idx < 16 {
        let code = ((bits >> (idx * 2)) & 3) as usize;
        pixels[idx] = colors[code];
        idx += 1;
    }

    pixels
}

fn rgb565(value: u16) -> [u8; 4] {
    let r = ((value >> 11) & 31) as u8;
    let g = ((value >> 5) & 63) as u8;
    let b = (value & 31) as u8;

    [r * 255 / 31, g * 255 / 63, b * 255 / 31, 255]
}

fn lerp_color(a: [u8; 4], b: [u8; 4], num: u16, den: u16) -> [u8; 4] {
    [
        ((u16::from(a[0]) * (den - num) + u16::from(b[0]) * num) / den) as u8,
        ((u16::from(a[1]) * (den - num) + u16::from(b[1]) * num) / den) as u8,
        ((u16::from(a[2]) * (den - num) + u16::from(b[2]) * num) / den) as u8,
        255,
    ]
}

fn read_text(source: &FileSource, path: &str) -> Option<String> {
    let bytes = source.read(path)?;

    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn normalize_path(path: &str) -> String {
    let mut out = String::new();

    for ch in path.chars() {
        if ch == '\\' {
            out.push('/');
        } else {
            out.push(ch.to_ascii_lowercase());
        }
    }

    while out.starts_with("./") {
        out.replace_range(..2, "");
    }

    out
}

pub fn material_key(name: &str) -> String {
    let mut key = normalize_path(name);

    if let Some(stripped) = key.strip_prefix("materials/") {
        key = stripped.to_string();
    }

    if let Some(stripped) = key.strip_suffix(".vmt") {
        key = stripped.to_string();
    }

    key
}

pub fn vmt_path(name: &str) -> String {
    format!("materials/{}.vmt", material_key(name))
}

pub fn vtf_path(name: &str) -> String {
    let mut key = normalize_path(name);

    if let Some(stripped) = key.strip_prefix("materials/") {
        key = stripped.to_string();
    }

    if let Some(stripped) = key.strip_suffix(".vtf") {
        key = stripped.to_string();
    }

    format!("materials/{key}.vtf")
}

fn loose_file(key: &str) -> Option<Vec<u8>> {
    let mut idx = 0;
    let roots = game_roots();

    while idx < roots.len() {
        let path = roots[idx].join(key);

        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }

        idx += 1;
    }

    None
}

fn game_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(value) = std::env::var("SOURCE_GAME") {
        if !value.is_empty() {
            roots.push(expand_home(value.trim()));
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("tf"));
        roots.push(cwd.clone());
    }

    roots
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }

    PathBuf::from(path)
}

fn home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }

    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.is_empty() {
            return PathBuf::from(profile);
        }
    }

    PathBuf::from(".")
}

fn mount_vpks() -> Vec<Vpk> {
    let mut archives = Vec::new();
    let roots = game_roots();
    let mut idx = 0;

    while idx < roots.len() {
        if !roots[idx].is_dir() {
            log::warn!("[mat] game folder {} was not found", roots[idx].display());
        }

        collect_vpks(&roots[idx], 0, &mut archives);
        idx += 1;
    }

    if archives.is_empty() {
        log::warn!("[mat] no vpk archives were mounted");
    }

    archives
}

fn collect_vpks(dir: &Path, depth: u32, archives: &mut Vec<Vpk>) {
    if depth > 3 {
        return;
    }

    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in read.flatten() {
        let path = entry.path();

        if path.is_dir() {
            collect_vpks(&path, depth + 1, archives);

            continue;
        }

        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if name.ends_with("_dir.vpk") {
            match Vpk::open(&path) {
                Ok(vpk) => {
                    log::info!("[mat] vpk {} ({} files)", path.display(), vpk.len());
                    archives.push(vpk);
                }
                Err(err) => log::warn!("[mat] vpk {}: {err}", path.display()),
            }
        }
    }
}

pub fn sky_paths(name: &str) -> [String; 6] {
    let key = material_key(name);
    let sides = ["rt", "lf", "bk", "ft", "up", "dn"];
    let mut paths = [
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ];
    let mut idx = 0;

    while idx < 6 {
        paths[idx] = format!("materials/skybox/{key}{}.vtf", sides[idx]);
        idx += 1;
    }

    paths
}

pub fn read_all(reader: &mut dyn Read) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();

    reader.read_to_end(&mut bytes).ok()?;

    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_in_source_game_expands() {
        let home = std::env::var("HOME").unwrap_or_else(|_| std::env::var("USERPROFILE").unwrap());
        let path = expand_home("~/Library/Application Support/GarrysMod");

        assert_eq!(
            path,
            PathBuf::from(home).join("Library/Application Support/GarrysMod")
        );
        assert_eq!(expand_home("/tmp/tf"), PathBuf::from("/tmp/tf"));
    }

    #[test]
    fn material_paths_drop_prefixes() {
        assert_eq!(material_key(r"Materials\Brick\Wall.vmt"), "brick/wall");
        assert_eq!(vmt_path("brick/wall"), "materials/brick/wall.vmt");
        assert_eq!(
            vtf_path("materials/brick/wall.vtf"),
            "materials/brick/wall.vtf"
        );
    }

    #[test]
    fn patch_material_keeps_the_base_shader() {
        let mut source = FileSource {
            pak: HashMap::new(),
            vpks: Vec::new(),
        };
        source.pak.insert(
            "materials/base.vmt".to_string(),
            b"LightmappedGeneric\n{\n\"$basetexture\" \"brick/wall\"\n}\n".to_vec(),
        );
        source.pak.insert(
            "materials/custom.vmt".to_string(),
            b"patch\n{\n\"include\" \"materials/base.vmt\"\n\"replace\"\n{\n\"$basetexture\" \"brick/other\"\n}\n}\n"
                .to_vec(),
        );
        let mut bank = MaterialBank::new(source);
        let id = bank.load("custom");
        let material = &bank.materials()[id as usize];

        assert_eq!(material.gpu.params[0], MODE_LIGHT);
        assert_eq!(material.base.format, PixelFormat::Rgba8);
        assert_eq!(&material.base.bytes[..4], &[255, 0, 255, 255]);
    }

    #[test]
    fn bgra_pixels_become_rgba() {
        let image = image_from_raw(ImageFormat::Bgra8888, 1, 1, &[1, 2, 3, 4]);

        assert_eq!(image.bytes, vec![3, 2, 1, 4]);
    }
}
