use std::sync::Arc;

#[derive(Clone)]
pub struct SkinGroup {
    pub key: u64,
    pub vertices: Arc<[f32]>,
    pub indices: Arc<[u32]>,
    pub albedo: Arc<[u8]>,
    pub albedo_w: u32,
    pub albedo_h: u32,
    pub bones: u32,
    pub instances: Arc<[f32]>,
    pub palette: Arc<[f32]>,
    pub palette_w: u32,
    pub palette_h: u32,
}

#[derive(Clone, Default)]
pub struct SkinBatch {
    pub groups: Vec<SkinGroup>,
}
