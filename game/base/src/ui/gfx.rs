use crate::ui::voxel::SceneView;
use crate::world::surface::CpuImage;
use std::marker::PhantomData;

pub const KIND_NONE: u32 = 0;
pub const KIND_SHADER: u32 = 1;
pub const KIND_TEXTURE: u32 = 2;
pub const KIND_BUFFER: u32 = 3;
pub const KIND_SAMPLER: u32 = 4;
pub const KIND_PIPELINE: u32 = 5;
pub const KIND_TARGET: u32 = 6;
pub const KIND_MESH: u32 = 7;

pub const IDX_MESH: u32 = 1;
pub const IDX_COLOR: u32 = 2;
pub const IDX_TEXT: u32 = 3;
pub const IDX_SKINNED: u32 = 4;
pub const IDX_WHITE: u32 = 1;
pub const IDX_FLAT: u32 = 2;
pub const IDX_WRAP: u32 = 1;
pub const IDX_CLAMP: u32 = 2;

pub const MESH_FLOATS: usize = 9;
pub const SCREEN_FLOATS: usize = 8;

const INDEX_BITS: u32 = 18;
const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;
const GENERATION_MASK: u32 = (1 << 11) - 1;
const FLAG_BUILTIN: u8 = 1;
const FLAG_DOOMED: u8 = 2;

#[inline]
pub const fn pack(kind: u32, generation: u32, index: u32) -> u32 {
    (kind << 29) | ((generation & GENERATION_MASK) << INDEX_BITS) | (index & INDEX_MASK)
}

#[inline]
pub const fn kind_of(id: u32) -> u32 {
    id >> 29
}

#[inline]
pub const fn generation_of(id: u32) -> u32 {
    (id >> INDEX_BITS) & GENERATION_MASK
}

#[inline]
pub const fn index_of(id: u32) -> u32 {
    id & INDEX_MASK
}

pub const SHADER_MESH: u32 = pack(KIND_SHADER, 1, IDX_MESH);
pub const SHADER_COLOR: u32 = pack(KIND_SHADER, 1, IDX_COLOR);
pub const SHADER_TEXT: u32 = pack(KIND_SHADER, 1, IDX_TEXT);
pub const SHADER_SKINNED: u32 = pack(KIND_SHADER, 1, IDX_SKINNED);
pub const PIPE_MESH: u32 = pack(KIND_PIPELINE, 1, IDX_MESH);
pub const PIPE_COLOR: u32 = pack(KIND_PIPELINE, 1, IDX_COLOR);
pub const PIPE_TEXT: u32 = pack(KIND_PIPELINE, 1, IDX_TEXT);
pub const PIPE_SKINNED: u32 = pack(KIND_PIPELINE, 1, IDX_SKINNED);
pub const TEX_WHITE: u32 = pack(KIND_TEXTURE, 1, IDX_WHITE);
pub const TEX_FLAT: u32 = pack(KIND_TEXTURE, 1, IDX_FLAT);
pub const SAMP_WRAP: u32 = pack(KIND_SAMPLER, 1, IDX_WRAP);
pub const SAMP_CLAMP: u32 = pack(KIND_SAMPLER, 1, IDX_CLAMP);

pub fn builtin_shader(name: &str) -> u32 {
    match name {
        "mesh" => SHADER_MESH,
        "color" => SHADER_COLOR,
        "text" => SHADER_TEXT,
        "skinned" => SHADER_SKINNED,
        _ => 0,
    }
}

pub fn builtin_pipeline(name: &str) -> u32 {
    match name {
        "mesh" => PIPE_MESH,
        "color" => PIPE_COLOR,
        "text" => PIPE_TEXT,
        "skinned" => PIPE_SKINNED,
        _ => 0,
    }
}

pub fn builtin_texture(name: &str) -> u32 {
    match name {
        "white" => TEX_WHITE,
        "flat" => TEX_FLAT,
        _ => 0,
    }
}

pub fn builtin_sampler(name: &str) -> u32 {
    match name {
        "wrap" => SAMP_WRAP,
        "clamp" => SAMP_CLAMP,
        _ => 0,
    }
}

pub fn is_builtin(id: u32) -> bool {
    if generation_of(id) != 1 || index_of(id) == 0 {
        return false;
    }

    let index = index_of(id);

    match kind_of(id) {
        KIND_SHADER | KIND_PIPELINE => index <= IDX_SKINNED,
        KIND_TEXTURE => index <= IDX_FLAT,
        KIND_SAMPLER => index <= IDX_CLAMP,
        _ => false,
    }
}

fn next_generation(generation: u16) -> u16 {
    match (generation.wrapping_add(1)) & (GENERATION_MASK as u16) {
        0 => 1,
        next => next,
    }
}

fn builtin_count(kind: u32) -> usize {
    match kind {
        KIND_SHADER | KIND_PIPELINE => IDX_SKINNED as usize,
        KIND_TEXTURE => IDX_FLAT as usize,
        KIND_SAMPLER => IDX_CLAMP as usize,
        _ => 0,
    }
}

struct Meta {
    generation: u16,
    flags: u8,
}

pub struct Book {
    slots: [Vec<Meta>; 8],
    free: [Vec<u32>; 8],
}

impl Book {
    pub fn new() -> Self {
        let mut book = Self {
            slots: Default::default(),
            free: Default::default(),
        };
        let mut kind = 0;

        while kind < 8 {
            book.slots[kind].push(Meta {
                generation: 0,
                flags: 0,
            });
            let count = builtin_count(kind as u32);
            let mut idx = 0;

            while idx < count {
                book.slots[kind].push(Meta {
                    generation: 1,
                    flags: FLAG_BUILTIN,
                });
                idx += 1;
            }

            kind += 1;
        }

        book
    }

    pub fn reset(&mut self) {
        let mut kind = 0;

        while kind < 8 {
            let keep = builtin_count(kind as u32) + 1;
            self.slots[kind].truncate(keep);
            self.free[kind].clear();
            let mut idx = 1;

            while idx < keep {
                self.slots[kind][idx].generation = 1;
                self.slots[kind][idx].flags = FLAG_BUILTIN;
                idx += 1;
            }

            kind += 1;
        }
    }

    pub fn alloc(&mut self, kind: u32) -> u32 {
        let kind_idx = kind as usize;

        if kind_idx >= self.slots.len() {
            return 0;
        }

        if let Some(index) = self.free[kind_idx].pop() {
            let slot = &mut self.slots[kind_idx][index as usize];
            slot.flags = 0;

            return pack(kind, slot.generation as u32, index);
        }

        let index = self.slots[kind_idx].len() as u32;

        if index > INDEX_MASK {
            return 0;
        }

        self.slots[kind_idx].push(Meta {
            generation: 1,
            flags: 0,
        });

        pack(kind, 1, index)
    }

    pub fn live(&self, id: u32) -> bool {
        if id == 0 {
            return false;
        }

        let kind = kind_of(id) as usize;
        let index = index_of(id) as usize;
        let Some(slot) = self.slots.get(kind).and_then(|slots| slots.get(index)) else {
            return false;
        };

        slot.generation == generation_of(id) as u16 && slot.flags & FLAG_DOOMED == 0
    }

    pub fn doom(&mut self, id: u32) -> bool {
        if is_builtin(id) {
            return false;
        }

        let kind = kind_of(id) as usize;
        let index = index_of(id) as usize;
        let Some(slot) = self
            .slots
            .get_mut(kind)
            .and_then(|slots| slots.get_mut(index))
        else {
            return false;
        };

        if slot.generation != generation_of(id) as u16 || slot.flags & FLAG_DOOMED != 0 {
            return false;
        }

        if slot.flags & FLAG_BUILTIN != 0 {
            return false;
        }

        slot.flags |= FLAG_DOOMED;

        true
    }

    pub fn recycle(&mut self, id: u32) {
        let kind = kind_of(id) as usize;
        let index = index_of(id) as usize;
        let Some(slot) = self
            .slots
            .get_mut(kind)
            .and_then(|slots| slots.get_mut(index))
        else {
            return;
        };

        if slot.generation != generation_of(id) as u16 || slot.flags & FLAG_DOOMED == 0 {
            return;
        }

        slot.generation = next_generation(slot.generation);
        slot.flags = 0;
        self.free[kind].push(index as u32);
    }
}

pub enum GfxKind<G, M, D11, D12, V> {
    #[cfg(not(target_os = "ios"))]
    OpenGL(G),
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    Metal(M),
    #[cfg(windows)]
    D3D11(D11),
    #[cfg(windows)]
    D3D12(D12),
    #[cfg(not(target_os = "ios"))]
    Vulkan(V),
    _Host(PhantomData<(G, M, D11, D12, V)>),
}

impl<G: Clone, M: Clone, D11: Clone, D12: Clone, V: Clone> Clone for GfxKind<G, M, D11, D12, V> {
    fn clone(&self) -> Self {
        match self {
            #[cfg(not(target_os = "ios"))]
            GfxKind::OpenGL(value) => GfxKind::OpenGL(value.clone()),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            GfxKind::Metal(value) => GfxKind::Metal(value.clone()),
            #[cfg(windows)]
            GfxKind::D3D11(value) => GfxKind::D3D11(value.clone()),
            #[cfg(windows)]
            GfxKind::D3D12(value) => GfxKind::D3D12(value.clone()),
            #[cfg(not(target_os = "ios"))]
            GfxKind::Vulkan(value) => GfxKind::Vulkan(value.clone()),
            GfxKind::_Host(_) => GfxKind::_Host(PhantomData),
        }
    }
}

pub struct Gfx<G, M, D11, D12, V> {
    pub kind: GfxKind<G, M, D11, D12, V>,
    _keep: PhantomData<(G, M, D11, D12, V)>,
}

impl<G: Clone, M: Clone, D11: Clone, D12: Clone, V: Clone> Clone for Gfx<G, M, D11, D12, V> {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind.clone(),
            _keep: PhantomData,
        }
    }
}

impl<G, M, D11, D12, V> Gfx<G, M, D11, D12, V> {
    fn new(kind: GfxKind<G, M, D11, D12, V>) -> Self {
        Self {
            kind,
            _keep: PhantomData,
        }
    }

    #[cfg(not(target_os = "ios"))]
    pub fn opengl(value: G) -> Self {
        Self::new(GfxKind::OpenGL(value))
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn metal(value: M) -> Self {
        Self::new(GfxKind::Metal(value))
    }

    #[cfg(windows)]
    pub fn d3d11(value: D11) -> Self {
        Self::new(GfxKind::D3D11(value))
    }

    #[cfg(windows)]
    pub fn d3d12(value: D12) -> Self {
        Self::new(GfxKind::D3D12(value))
    }

    #[cfg(not(target_os = "ios"))]
    pub fn vulkan(value: V) -> Self {
        Self::new(GfxKind::Vulkan(value))
    }

    #[cfg(not(target_os = "ios"))]
    pub fn as_opengl(&self) -> Option<&G> {
        match &self.kind {
            GfxKind::OpenGL(value) => Some(value),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            GfxKind::Metal(_) => None,
            #[cfg(windows)]
            GfxKind::D3D11(_) => None,
            #[cfg(windows)]
            GfxKind::D3D12(_) => None,
            GfxKind::Vulkan(_) => None,
            GfxKind::_Host(_) => None,
        }
    }

    #[cfg(not(target_os = "ios"))]
    pub fn into_opengl(self) -> Option<G> {
        match self.kind {
            GfxKind::OpenGL(value) => Some(value),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            GfxKind::Metal(_) => None,
            #[cfg(windows)]
            GfxKind::D3D11(_) => None,
            #[cfg(windows)]
            GfxKind::D3D12(_) => None,
            GfxKind::Vulkan(_) => None,
            GfxKind::_Host(_) => None,
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn as_metal(&self) -> Option<&M> {
        match &self.kind {
            GfxKind::Metal(value) => Some(value),
            #[cfg(not(target_os = "ios"))]
            GfxKind::OpenGL(_) => None,
            #[cfg(windows)]
            GfxKind::D3D11(_) => None,
            #[cfg(windows)]
            GfxKind::D3D12(_) => None,
            #[cfg(not(target_os = "ios"))]
            GfxKind::Vulkan(_) => None,
            GfxKind::_Host(_) => None,
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn into_metal(self) -> Option<M> {
        match self.kind {
            GfxKind::Metal(value) => Some(value),
            #[cfg(not(target_os = "ios"))]
            GfxKind::OpenGL(_) => None,
            #[cfg(windows)]
            GfxKind::D3D11(_) => None,
            #[cfg(windows)]
            GfxKind::D3D12(_) => None,
            #[cfg(not(target_os = "ios"))]
            GfxKind::Vulkan(_) => None,
            GfxKind::_Host(_) => None,
        }
    }

    #[cfg(windows)]
    pub fn as_d3d11(&self) -> Option<&D11> {
        match &self.kind {
            GfxKind::D3D11(value) => Some(value),
            #[cfg(not(target_os = "ios"))]
            GfxKind::OpenGL(_) => None,
            GfxKind::D3D12(_) => None,
            #[cfg(not(target_os = "ios"))]
            GfxKind::Vulkan(_) => None,
            GfxKind::_Host(_) => None,
        }
    }

    #[cfg(windows)]
    pub fn into_d3d11(self) -> Option<D11> {
        match self.kind {
            GfxKind::D3D11(value) => Some(value),
            #[cfg(not(target_os = "ios"))]
            GfxKind::OpenGL(_) => None,
            GfxKind::D3D12(_) => None,
            #[cfg(not(target_os = "ios"))]
            GfxKind::Vulkan(_) => None,
            GfxKind::_Host(_) => None,
        }
    }

    #[cfg(windows)]
    pub fn as_d3d12(&self) -> Option<&D12> {
        match &self.kind {
            GfxKind::D3D12(value) => Some(value),
            #[cfg(not(target_os = "ios"))]
            GfxKind::OpenGL(_) => None,
            GfxKind::D3D11(_) => None,
            #[cfg(not(target_os = "ios"))]
            GfxKind::Vulkan(_) => None,
            GfxKind::_Host(_) => None,
        }
    }

    #[cfg(windows)]
    pub fn into_d3d12(self) -> Option<D12> {
        match self.kind {
            GfxKind::D3D12(value) => Some(value),
            #[cfg(not(target_os = "ios"))]
            GfxKind::OpenGL(_) => None,
            GfxKind::D3D11(_) => None,
            #[cfg(not(target_os = "ios"))]
            GfxKind::Vulkan(_) => None,
            GfxKind::_Host(_) => None,
        }
    }

    #[cfg(not(target_os = "ios"))]
    pub fn as_vulkan(&self) -> Option<&V> {
        match &self.kind {
            GfxKind::Vulkan(value) => Some(value),
            GfxKind::OpenGL(_) => None,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            GfxKind::Metal(_) => None,
            #[cfg(windows)]
            GfxKind::D3D11(_) => None,
            #[cfg(windows)]
            GfxKind::D3D12(_) => None,
            GfxKind::_Host(_) => None,
        }
    }

    #[cfg(not(target_os = "ios"))]
    pub fn into_vulkan(self) -> Option<V> {
        match self.kind {
            GfxKind::Vulkan(value) => Some(value),
            GfxKind::OpenGL(_) => None,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            GfxKind::Metal(_) => None,
            #[cfg(windows)]
            GfxKind::D3D11(_) => None,
            #[cfg(windows)]
            GfxKind::D3D12(_) => None,
            GfxKind::_Host(_) => None,
        }
    }
}

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct GlShader {
    pub vs: glow::Shader,
    pub fs: glow::Shader,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct GlShader;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct GlPipeline {
    pub program: glow::Program,
    pub stride: u8,
    pub depth: bool,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct GlPipeline;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct GlTexture {
    pub name: glow::Texture,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct GlTexture;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct GlBuffer {
    pub name: glow::Buffer,
    pub bytes: u32,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct GlBuffer;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct GlSampler {
    pub name: glow::Sampler,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct GlSampler;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct GlTarget {
    pub frame: glow::Framebuffer,
    pub color: glow::Texture,
    pub depth: glow::Renderbuffer,
    pub width: i32,
    pub height: i32,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct GlTarget;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct GlMesh {
    pub vao: glow::VertexArray,
    pub vbo: glow::Buffer,
    pub floats: i32,
    pub screen: bool,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct GlMesh;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Clone)]
pub struct MtlShader {
    pub library: metal::Library,
    pub vs: metal::Function,
    pub fs: metal::Function,
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[derive(Clone, Copy)]
pub struct MtlShader;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Clone)]
pub struct MtlPipeline {
    pub state: metal::RenderPipelineState,
    pub stride: u8,
    pub depth: bool,
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[derive(Clone, Copy)]
pub struct MtlPipeline;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Clone)]
pub struct MtlTexture {
    pub texture: metal::Texture,
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[derive(Clone, Copy)]
pub struct MtlTexture;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Clone)]
pub struct MtlBuffer {
    pub buffer: metal::Buffer,
    pub bytes: u64,
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[derive(Clone, Copy)]
pub struct MtlBuffer;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Clone)]
pub struct MtlSampler {
    pub state: metal::SamplerState,
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[derive(Clone, Copy)]
pub struct MtlSampler;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Clone)]
pub struct MtlTarget {
    pub color: metal::Texture,
    pub depth: metal::Texture,
    pub width: u64,
    pub height: u64,
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[derive(Clone, Copy)]
pub struct MtlTarget;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Clone)]
pub struct MtlMesh {
    pub buffer: metal::Buffer,
    pub floats: u64,
    pub screen: bool,
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[derive(Clone, Copy)]
pub struct MtlMesh;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx11Shader {
    pub vs: windows::Win32::Graphics::Direct3D::ID3DBlob,
    pub ps: windows::Win32::Graphics::Direct3D::ID3DBlob,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx11Shader;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx11Pipeline {
    pub vs: windows::Win32::Graphics::Direct3D11::ID3D11VertexShader,
    pub ps: windows::Win32::Graphics::Direct3D11::ID3D11PixelShader,
    pub layout: windows::Win32::Graphics::Direct3D11::ID3D11InputLayout,
    pub stride: u8,
    pub depth: bool,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx11Pipeline;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx11Texture {
    pub texture: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    pub view: windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx11Texture;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx11Buffer {
    pub buffer: windows::Win32::Graphics::Direct3D11::ID3D11Buffer,
    pub bytes: u32,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx11Buffer;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx11Sampler {
    pub state: windows::Win32::Graphics::Direct3D11::ID3D11SamplerState,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx11Sampler;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx11Target {
    pub color: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    pub view: windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
    pub rtv: windows::Win32::Graphics::Direct3D11::ID3D11RenderTargetView,
    pub depth: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    pub dsv: windows::Win32::Graphics::Direct3D11::ID3D11DepthStencilView,
    pub width: u32,
    pub height: u32,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx11Target;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx11Mesh {
    pub buffer: windows::Win32::Graphics::Direct3D11::ID3D11Buffer,
    pub floats: u32,
    pub screen: bool,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx11Mesh;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx12Shader {
    pub vs: windows::Win32::Graphics::Direct3D::ID3DBlob,
    pub ps: windows::Win32::Graphics::Direct3D::ID3DBlob,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx12Shader;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx12Pipeline {
    pub root: windows::Win32::Graphics::Direct3D12::ID3D12RootSignature,
    pub state: windows::Win32::Graphics::Direct3D12::ID3D12PipelineState,
    pub stride: u8,
    pub depth: bool,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx12Pipeline;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx12Texture {
    pub resource: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx12Texture;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx12Buffer {
    pub resource: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pub bytes: u64,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx12Buffer;

#[cfg(windows)]
#[derive(Clone, Copy)]
pub struct Dx12Sampler {
    pub linear: bool,
    pub repeat: bool,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx12Sampler;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx12Target {
    pub color: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pub depth: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pub width: u32,
    pub height: u32,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx12Target;

#[cfg(windows)]
#[derive(Clone)]
pub struct Dx12Mesh {
    pub resource: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pub floats: u32,
    pub screen: bool,
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
pub struct Dx12Mesh;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct VkShader {
    pub vs: ash::vk::ShaderModule,
    pub fs: ash::vk::ShaderModule,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct VkShader;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct VkPipeline {
    pub pipeline: ash::vk::Pipeline,
    pub layout: ash::vk::PipelineLayout,
    pub stride: u8,
    pub depth: bool,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct VkPipeline;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct VkTexture {
    pub image: ash::vk::Image,
    pub memory: ash::vk::DeviceMemory,
    pub view: ash::vk::ImageView,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct VkTexture;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct VkBuffer {
    pub buffer: ash::vk::Buffer,
    pub memory: ash::vk::DeviceMemory,
    pub bytes: u64,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct VkBuffer;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct VkSampler {
    pub sampler: ash::vk::Sampler,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct VkSampler;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct VkTarget {
    pub color: ash::vk::Image,
    pub color_memory: ash::vk::DeviceMemory,
    pub color_view: ash::vk::ImageView,
    pub depth: ash::vk::Image,
    pub depth_memory: ash::vk::DeviceMemory,
    pub depth_view: ash::vk::ImageView,
    pub frame: ash::vk::Framebuffer,
    pub width: u32,
    pub height: u32,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct VkTarget;

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy)]
pub struct VkMesh {
    pub buffer: ash::vk::Buffer,
    pub memory: ash::vk::DeviceMemory,
    pub floats: u32,
    pub screen: bool,
}

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
pub struct VkMesh;

pub type Shader = Gfx<GlShader, MtlShader, Dx11Shader, Dx12Shader, VkShader>;
pub type Pipeline = Gfx<GlPipeline, MtlPipeline, Dx11Pipeline, Dx12Pipeline, VkPipeline>;
pub type Texture = Gfx<GlTexture, MtlTexture, Dx11Texture, Dx12Texture, VkTexture>;
pub type Buffer = Gfx<GlBuffer, MtlBuffer, Dx11Buffer, Dx12Buffer, VkBuffer>;
pub type Sampler = Gfx<GlSampler, MtlSampler, Dx11Sampler, Dx12Sampler, VkSampler>;
pub type Target = Gfx<GlTarget, MtlTarget, Dx11Target, Dx12Target, VkTarget>;
pub type Mesh = Gfx<GlMesh, MtlMesh, Dx11Mesh, Dx12Mesh, VkMesh>;

pub struct Slot<T> {
    pub generation: u16,
    pub owned: bool,
    pub builtin: bool,
    pub screen: bool,
    pub stride: u8,
    pub material: Option<String>,
    pub item: T,
}

pub struct Store {
    shaders: Vec<Option<Slot<Shader>>>,
    textures: Vec<Option<Slot<Texture>>>,
    buffers: Vec<Option<Slot<Buffer>>>,
    samplers: Vec<Option<Slot<Sampler>>>,
    pipelines: Vec<Option<Slot<Pipeline>>>,
    targets: Vec<Option<Slot<Target>>>,
    meshes: Vec<Option<Slot<Mesh>>>,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        Self {
            shaders: vec![None],
            textures: vec![None],
            buffers: vec![None],
            samplers: vec![None],
            pipelines: vec![None],
            targets: vec![None],
            meshes: vec![None],
        }
    }

    pub fn shader(&self, id: u32) -> Option<&Shader> {
        Some(&self.slot(&self.shaders, id)?.item)
    }

    pub fn texture_slot(&self, id: u32) -> Option<&Slot<Texture>> {
        self.slot(&self.textures, id)
    }

    pub fn texture(&self, id: u32) -> Option<&Texture> {
        Some(&self.texture_slot(id)?.item)
    }

    pub fn buffer(&self, id: u32) -> Option<&Buffer> {
        Some(&self.slot(&self.buffers, id)?.item)
    }

    pub fn sampler(&self, id: u32) -> Option<&Sampler> {
        Some(&self.slot(&self.samplers, id)?.item)
    }

    pub fn pipeline_slot(&self, id: u32) -> Option<&Slot<Pipeline>> {
        self.slot(&self.pipelines, id)
    }

    pub fn pipeline(&self, id: u32) -> Option<&Pipeline> {
        Some(&self.pipeline_slot(id)?.item)
    }

    pub fn target(&self, id: u32) -> Option<&Target> {
        Some(&self.slot(&self.targets, id)?.item)
    }

    pub fn mesh_slot(&self, id: u32) -> Option<&Slot<Mesh>> {
        self.slot(&self.meshes, id)
    }

    pub fn mesh(&self, id: u32) -> Option<&Mesh> {
        Some(&self.mesh_slot(id)?.item)
    }

    fn slot<'a, T>(&self, slots: &'a [Option<Slot<T>>], id: u32) -> Option<&'a Slot<T>> {
        let slot = slots.get(index_of(id) as usize)?.as_ref()?;

        if slot.generation != generation_of(id) as u16 {
            return None;
        }

        Some(slot)
    }

    pub fn put_shader(&mut self, id: u32, item: Shader, owned: bool, builtin: bool) {
        write_slot(&mut self.shaders, id, item, owned, builtin, false, 0, None);
    }

    pub fn put_texture(
        &mut self,
        id: u32,
        item: Texture,
        owned: bool,
        builtin: bool,
        material: Option<String>,
    ) {
        write_slot(
            &mut self.textures,
            id,
            item,
            owned,
            builtin,
            false,
            0,
            material,
        );
    }

    pub fn put_buffer(&mut self, id: u32, item: Buffer, owned: bool) {
        write_slot(&mut self.buffers, id, item, owned, false, false, 0, None);
    }

    pub fn put_sampler(&mut self, id: u32, item: Sampler, owned: bool, builtin: bool) {
        write_slot(&mut self.samplers, id, item, owned, builtin, false, 0, None);
    }

    pub fn put_pipeline(
        &mut self,
        id: u32,
        item: Pipeline,
        owned: bool,
        builtin: bool,
        screen: bool,
        stride: u8,
    ) {
        write_slot(
            &mut self.pipelines,
            id,
            item,
            owned,
            builtin,
            screen,
            stride,
            None,
        );
    }

    pub fn put_target(&mut self, id: u32, item: Target, owned: bool) {
        write_slot(&mut self.targets, id, item, owned, false, false, 0, None);
    }

    pub fn put_mesh(&mut self, id: u32, item: Mesh, owned: bool, screen: bool) {
        let stride = if screen {
            SCREEN_FLOATS as u8
        } else {
            MESH_FLOATS as u8
        };
        write_slot(
            &mut self.meshes,
            id,
            item,
            owned,
            false,
            screen,
            stride,
            None,
        );
    }

    pub fn take_shader(&mut self, id: u32) -> Option<Slot<Shader>> {
        read_take(&mut self.shaders, id)
    }

    pub fn take_texture(&mut self, id: u32) -> Option<Slot<Texture>> {
        read_take(&mut self.textures, id)
    }

    pub fn take_buffer(&mut self, id: u32) -> Option<Slot<Buffer>> {
        read_take(&mut self.buffers, id)
    }

    pub fn take_sampler(&mut self, id: u32) -> Option<Slot<Sampler>> {
        read_take(&mut self.samplers, id)
    }

    pub fn take_pipeline(&mut self, id: u32) -> Option<Slot<Pipeline>> {
        read_take(&mut self.pipelines, id)
    }

    pub fn take_target(&mut self, id: u32) -> Option<Slot<Target>> {
        read_take(&mut self.targets, id)
    }

    pub fn take_mesh(&mut self, id: u32) -> Option<Slot<Mesh>> {
        read_take(&mut self.meshes, id)
    }

    pub fn drain_shaders(&mut self) -> Vec<Slot<Shader>> {
        drain_owned(&mut self.shaders)
    }

    pub fn drain_textures(&mut self) -> Vec<Slot<Texture>> {
        drain_owned(&mut self.textures)
    }

    pub fn drain_buffers(&mut self) -> Vec<Slot<Buffer>> {
        drain_owned(&mut self.buffers)
    }

    pub fn drain_samplers(&mut self) -> Vec<Slot<Sampler>> {
        drain_owned(&mut self.samplers)
    }

    pub fn drain_pipelines(&mut self) -> Vec<Slot<Pipeline>> {
        drain_owned(&mut self.pipelines)
    }

    pub fn drain_targets(&mut self) -> Vec<Slot<Target>> {
        drain_owned(&mut self.targets)
    }

    pub fn drain_meshes(&mut self) -> Vec<Slot<Mesh>> {
        drain_owned(&mut self.meshes)
    }
}

fn write_slot<T>(
    slots: &mut Vec<Option<Slot<T>>>,
    id: u32,
    item: T,
    owned: bool,
    builtin: bool,
    screen: bool,
    stride: u8,
    material: Option<String>,
) {
    let index = index_of(id) as usize;

    if slots.len() <= index {
        slots.resize_with(index + 1, || None);
    }

    slots[index] = Some(Slot {
        generation: generation_of(id) as u16,
        owned,
        builtin,
        screen,
        stride,
        material,
        item,
    });
}

fn read_take<T>(slots: &mut [Option<Slot<T>>], id: u32) -> Option<Slot<T>> {
    let slot = slots.get_mut(index_of(id) as usize)?;
    let current = slot.as_ref()?;

    if current.generation != generation_of(id) as u16 || current.builtin {
        return None;
    }

    slot.take()
}

fn drain_owned<T>(slots: &mut Vec<Option<Slot<T>>>) -> Vec<Slot<T>> {
    let mut owned = Vec::new();
    let mut idx = 0;

    while idx < slots.len() {
        if let Some(slot) = slots[idx].take() {
            if slot.owned {
                owned.push(slot);
            }
        }

        idx += 1;
    }

    owned
}

pub trait BackendGpu {
    fn make_shader(&mut self, wgsl: &str) -> Result<Shader, String>;
    fn make_texture(&mut self, image: &CpuImage) -> Result<Texture, String>;
    fn make_target(&mut self, width: u32, height: u32) -> Result<Target, String>;
    fn make_buffer(&mut self, bytes: &[u8]) -> Result<Buffer, String>;
    fn make_sampler(&mut self, linear: bool, repeat: bool) -> Result<Sampler, String>;
    fn make_pipeline(&mut self, shader: &Shader, screen: bool) -> Result<Pipeline, String>;
    fn make_mesh(&mut self, verts: &[f32], screen: bool) -> Result<Mesh, String>;
    fn destroy_shader(&mut self, shader: Shader);
    fn destroy_texture(&mut self, texture: Texture);
    fn destroy_buffer(&mut self, buffer: Buffer);
    fn destroy_sampler(&mut self, sampler: Sampler);
    fn destroy_pipeline(&mut self, pipeline: Pipeline);
    fn destroy_target(&mut self, target: Target);
    fn destroy_mesh(&mut self, mesh: Mesh);
    fn draw_mesh(
        &mut self,
        mesh: &Mesh,
        pipeline: &Pipeline,
        texture: Option<&Texture>,
        sampler: Option<&Sampler>,
        view: &SceneView,
    );
    fn draw_sprite(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        texture: Option<&Texture>,
        pipeline: Option<&Pipeline>,
        sampler: Option<&Sampler>,
    );
    fn draw_buffer(
        &mut self,
        buffer: &Buffer,
        pipeline: &Pipeline,
        texture: Option<&Texture>,
        sampler: Option<&Sampler>,
        view: &SceneView,
    );
    fn draw_screen(&mut self, verts: &[f32], texture: Option<&Texture>, sampler: Option<&Sampler>);
    fn draw_text_user(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        texture: Option<&Texture>,
        pipeline: Option<&Pipeline>,
        sampler: Option<&Sampler>,
    );
    fn set_target(&mut self, target: Option<&Target>);
    fn target_bound(&self) -> bool;
    fn update_buffer(&mut self, buffer: Buffer, bytes: &[u8]) -> Buffer;
    fn update_mesh(&mut self, mesh: Mesh, verts: &[f32]) -> Mesh;
    fn update_texture(&mut self, texture: Texture, image: &CpuImage) -> Texture;
    fn resize_target(&mut self, target: Target, width: u32, height: u32) -> Result<Target, String>;
    fn builtin_shader(&mut self, index: u32) -> Option<Shader>;
    fn builtin_pipeline(&mut self, index: u32) -> Option<Pipeline>;
    fn builtin_texture(&mut self, index: u32) -> Option<Texture>;
    fn builtin_sampler(&mut self, index: u32) -> Option<Sampler>;
    fn material_alias(&self, name: &str) -> Option<Texture>;
    fn target_color(&self, target: &Target) -> Option<Texture>;
    fn before_destroy(&mut self) {}
}

pub fn screen_quad(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> [f32; 48] {
    let corners = [
        (x, y, 0.0, 0.0),
        (x + w, y, 1.0, 0.0),
        (x, y + h, 0.0, 1.0),
        (x, y + h, 0.0, 1.0),
        (x + w, y, 1.0, 0.0),
        (x + w, y + h, 1.0, 1.0),
    ];
    let mut out = [0.0; 48];
    let mut idx = 0;
    let mut vert = 0;

    while vert < corners.len() {
        let (px, py, u, v) = corners[vert];
        out[idx] = px;
        out[idx + 1] = py;
        out[idx + 2] = u;
        out[idx + 3] = v;
        out[idx + 4] = color[0];
        out[idx + 5] = color[1];
        out[idx + 6] = color[2];
        out[idx + 7] = color[3];
        idx += SCREEN_FLOATS;
        vert += 1;
    }

    out
}

pub fn image_file(path: &str) -> Result<CpuImage, String> {
    let bytes = crate::fs::read(path)?;

    if path.ends_with(".png") || bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return image_png(&bytes);
    }

    if let Some(image) = crate::world::image_from_vtf(&bytes) {
        return Ok(image);
    }

    image_png(&bytes)
}

pub fn material_image(name: &str) -> Option<CpuImage> {
    crate::world::read_texture(name)
}

fn image_png(bytes: &[u8]) -> Result<CpuImage, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|err| err.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).map_err(|err| err.to_string())?;
    let pixels = rgba_png(
        &buf[..frame.buffer_size()],
        frame.width,
        frame.height,
        frame.color_type,
    )?;

    Ok(CpuImage {
        width: frame.width,
        height: frame.height,
        format: crate::world::surface::PixelFormat::Rgba8,
        bytes: pixels,
        mips: Vec::new(),
    })
}

fn rgba_png(
    samples: &[u8],
    width: u32,
    height: u32,
    color: png::ColorType,
) -> Result<Vec<u8>, String> {
    let count = (width as usize).saturating_mul(height as usize);
    let mut pixels = Vec::with_capacity(count * 4);

    match color {
        png::ColorType::Rgba => {
            if samples.len() < count * 4 {
                return Err("png".to_string());
            }

            pixels.extend_from_slice(&samples[..count * 4]);
        }
        png::ColorType::Rgb => {
            if samples.len() < count * 3 {
                return Err("png".to_string());
            }

            let mut idx = 0;

            while idx < count {
                let at = idx * 3;
                pixels.extend_from_slice(&[samples[at], samples[at + 1], samples[at + 2], 255]);
                idx += 1;
            }
        }
        png::ColorType::Grayscale => {
            if samples.len() < count {
                return Err("png".to_string());
            }

            let mut idx = 0;

            while idx < count {
                let value = samples[idx];
                pixels.extend_from_slice(&[value, value, value, 255]);
                idx += 1;
            }
        }
        png::ColorType::GrayscaleAlpha => {
            if samples.len() < count * 2 {
                return Err("png".to_string());
            }

            let mut idx = 0;

            while idx < count {
                let at = idx * 2;
                let value = samples[at];
                pixels.extend_from_slice(&[value, value, value, samples[at + 1]]);
                idx += 1;
            }
        }
        _ => return Err("png".to_string()),
    }

    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip() {
        let id = pack(KIND_TEXTURE, 7, 42);

        assert_eq!(kind_of(id), KIND_TEXTURE);
        assert_eq!(generation_of(id), 7);
        assert_eq!(index_of(id), 42);
        assert!(!is_builtin(id));
        assert!(is_builtin(SHADER_MESH));
        assert_eq!(builtin_shader("mesh"), SHADER_MESH);
        assert_eq!(builtin_shader("nope"), 0);
    }

    #[test]
    fn book_reuses_after_recycle() {
        let mut book = Book::new();
        let id = book.alloc(KIND_MESH);
        assert!(book.live(id));
        assert!(book.doom(id));
        assert!(!book.live(id));
        book.recycle(id);
        let next = book.alloc(KIND_MESH);
        assert_eq!(index_of(next), index_of(id));
        assert_ne!(generation_of(next), generation_of(id));
        assert!(!book.doom(SHADER_COLOR));
    }
}
