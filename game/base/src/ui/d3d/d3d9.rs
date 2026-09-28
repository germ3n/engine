use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Graphics::Direct3D9::*;
use windows::core::{HRESULT, Interface};
use winit::event_loop::EventLoop;
use winit::window::Window as WinitWindow;

use crate::ui::d3d::draw::{bytes_of, grow, open_desktop, push_outline, push_rect, Desktop, TextFrame};
use crate::ui::d3d::math::view_proj;
use crate::ui::d3d::shader::{self, blob_bytes};
use crate::ui::vr::{self, EyeViews, Headset, VrInput};
use crate::ui::voxel::SceneView;
use crate::ui::window::Window;
use crate::ui::Color;

const DEVICE_LOST: i32 = 0x88760868u32 as i32;
const DEVICE_NOT_RESET: i32 = 0x88760869u32 as i32;

pub struct D3D9Window {
    desktop: Desktop,
    device: IDirect3DDevice9,
    params: D3DPRESENT_PARAMETERS,
    mesh_vs: IDirect3DVertexShader9,
    mesh_ps: IDirect3DPixelShader9,
    mesh_decl: IDirect3DVertexDeclaration9,
    color_vs: IDirect3DVertexShader9,
    color_ps: IDirect3DPixelShader9,
    color_decl: IDirect3DVertexDeclaration9,
    text_vs: IDirect3DVertexShader9,
    text_ps: IDirect3DPixelShader9,
    text_decl: IDirect3DVertexDeclaration9,
    mesh: Option<GpuBuf>,
    ui_buf: Option<GpuBuf>,
    text_buf: Option<GpuBuf>,
    atlas: Option<IDirect3DTexture9>,
    atlas_size: (u32, u32),
    width: u32,
    height: u32,
    clear: [f32; 4],
    ui: Vec<f32>,
    text: TextFrame,
    mesh_vertices: u32,
    mesh_revision: u64,
    mesh_ready: bool,
    view: [f32; 16],
    draw_mesh: bool,
    vr: Option<Headset>,
    vr_failed: bool,
    vr_enable: bool,
    eyes: Option<Eyes9>,
    eye_views: Option<EyeViews>,
}

struct Eyes9 {
    width: u32,
    height: u32,
    color: [IDirect3DTexture9; 2],
    surface: [IDirect3DSurface9; 2],
    shared: [HANDLE; 2],
    depth: [IDirect3DSurface9; 2],
}

struct GpuBuf {
    buffer: IDirect3DVertexBuffer9,
    capacity: u32,
    stride: u32,
}

impl D3D9Window {
    pub fn try_new() -> Result<Self, String> {
        let desktop = open_desktop()?;
        let size = desktop.window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let (device, params) = create_device(desktop.hwnd, width, height)?;
        let mesh_vs = vertex_shader(&device, &shader::vs3(shader::MESH_SM3)?)?;
        let mesh_ps = pixel_shader(&device, &shader::ps3(shader::MESH_PS_SM3)?)?;
        let color_vs = vertex_shader(&device, &shader::vs3(shader::COLOR_SM3)?)?;
        let color_ps = pixel_shader(&device, &shader::ps3(shader::COLOR_PS_SM3)?)?;
        let text_vs = vertex_shader(&device, &shader::vs3(shader::TEXT_SM3)?)?;
        let text_ps = pixel_shader(&device, &shader::ps3(shader::TEXT_PS_SM3)?)?;
        let mesh_decl = declaration(&device, &mesh_decl())?;
        let color_decl = declaration(&device, &color_decl())?;
        let text_decl = declaration(&device, &text_decl())?;
        let text = TextFrame::new()?;

        Ok(Self {
            desktop,
            device,
            params,
            mesh_vs,
            mesh_ps,
            mesh_decl,
            color_vs,
            color_ps,
            color_decl,
            text_vs,
            text_ps,
            text_decl,
            mesh: None,
            ui_buf: None,
            text_buf: None,
            atlas: None,
            atlas_size: (0, 0),
            width,
            height,
            clear: [0.0, 0.0, 0.0, 1.0],
            ui: Vec::new(),
            text,
            mesh_vertices: 0,
            mesh_revision: 0,
            mesh_ready: false,
            view: [0.0; 16],
            draw_mesh: false,
            vr: None,
            vr_failed: false,
            vr_enable: false,
            eyes: None,
            eye_views: None,
        })
    }

    fn reset_device(&mut self) -> Result<(), String> {
        self.params.BackBufferWidth = self.width.max(1);
        self.params.BackBufferHeight = self.height.max(1);
        unsafe { self.device.Reset(&mut self.params).map_err(|err| err.to_string())? };
        self.width = self.params.BackBufferWidth.max(1);
        self.height = self.params.BackBufferHeight.max(1);

        Ok(())
    }

    fn ready(&mut self) -> bool {
        match unsafe { self.device.TestCooperativeLevel() } {
            Ok(()) => {
                if self.params.BackBufferWidth != self.width || self.params.BackBufferHeight != self.height {
                    return self.reset_device().is_ok();
                }

                true
            }
            Err(err) if err.code() == HRESULT(DEVICE_NOT_RESET) => self.reset_device().is_ok(),
            Err(_) => false,
        }
    }

    fn upload_mesh(&mut self, vertices: &[f32]) -> Result<(), String> {
        let bytes = bytes_of(vertices);
        self.mesh = Some(write_buffer(&self.device, self.mesh.take(), bytes, 24)?);

        Ok(())
    }

    fn sync_atlas(&mut self) -> Result<(), String> {
        if !self.text.dirty && self.atlas_size == self.text.size && self.atlas.is_some() {
            return Ok(());
        }

        if self.atlas_size != self.text.size || self.atlas.is_none() {
            self.atlas = Some(atlas_texture(&self.device, self.text.size.0, self.text.size.1)?);
            self.atlas_size = self.text.size;
        }

        let Some(texture) = self.atlas.as_ref() else {
            return Ok(());
        };
        unsafe {
            let mut locked = D3DLOCKED_RECT::default();
            texture.LockRect(0, &mut locked, std::ptr::null(), 0).map_err(|err| err.to_string())?;
            expand_bgra(&self.text.pixels, self.text.size.0, self.text.size.1, locked.pBits as *mut u8, locked.Pitch);
            texture.UnlockRect(0).map_err(|err| err.to_string())?;
        }
        self.text.dirty = false;

        Ok(())
    }

    fn draw_buffer(
        &self,
        vs: &IDirect3DVertexShader9,
        ps: &IDirect3DPixelShader9,
        decl: &IDirect3DVertexDeclaration9,
        constants: &[f32],
        buffer: &GpuBuf,
        vertices: u32,
        depth: bool,
        blend: bool,
        texture: Option<&IDirect3DTexture9>,
    ) {
        if vertices < 3 {
            return;
        }

        let vectors = ((constants.len() + 3) / 4) as u32;
        unsafe {
            let _ = self.device.SetVertexDeclaration(decl);
            let _ = self.device.SetVertexShader(vs);
            let _ = self.device.SetPixelShader(ps);
            let _ = self.device.SetVertexShaderConstantF(0, constants.as_ptr(), vectors);
            let _ = self.device.SetStreamSource(0, &buffer.buffer, 0, buffer.stride);
            let _ = self.device.SetRenderState(D3DRS_ZENABLE, if depth { 1 } else { 0 });
            let _ = self.device.SetRenderState(D3DRS_ZWRITEENABLE, if depth { 1 } else { 0 });
            let _ = self.device.SetRenderState(D3DRS_ZFUNC, D3DCMP_LESS.0 as u32);
            let _ = self.device.SetRenderState(D3DRS_CULLMODE, if depth { D3DCULL_CW.0 as u32 } else { D3DCULL_NONE.0 as u32 });
            let _ = self.device.SetRenderState(D3DRS_ALPHABLENDENABLE, if blend { 1 } else { 0 });
            let _ = self.device.SetRenderState(D3DRS_SRCBLEND, D3DBLEND_SRCALPHA.0 as u32);
            let _ = self.device.SetRenderState(D3DRS_DESTBLEND, D3DBLEND_INVSRCALPHA.0 as u32);
            match texture {
                Some(texture) => {
                    let _ = self.device.SetTexture(0, Into::<&IDirect3DBaseTexture9>::into(texture));
                }
                None => {
                    let _ = self.device.SetTexture(0, None::<&IDirect3DBaseTexture9>);
                }
            }
            let _ = self.device.DrawPrimitive(D3DPT_TRIANGLELIST, 0, vertices / 3);
        }
    }
}

impl Window for D3D9Window {
    fn create_window() -> Self {
        Self::try_new().expect("d3d9")
    }

    fn set_window_title(&mut self, title: &str) {
        self.desktop.window.set_title(title);
    }

    fn set_size(&mut self, w: u32, h: u32) {
        let _ = self.desktop.window.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
        self.width = w.max(1);
        self.height = h.max(1);

        match unsafe { self.device.TestCooperativeLevel() } {
            Err(err) if err.code() == HRESULT(DEVICE_LOST) => {}
            _ => {
                if let Err(err) = self.reset_device() {
                    println!("[gfx] d3d9 resize {err}");
                }
            }
        }
    }

    fn winit_window(&self) -> &WinitWindow {
        &self.desktop.window
    }

    fn take_event_loop(&mut self) -> EventLoop<()> {
        self.desktop.event_loop.take().expect("Event loop missing")
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        self.clear = [red, green, blue, 1.0];
        self.ui.clear();
        self.draw_mesh = false;
    }

    fn draw_colored_mesh(&mut self, vertices: &[f32], revision: u64, view: &SceneView) {
        self.view = view_proj(view);
        self.eye_views = vr::connect(&mut self.vr, &mut self.vr_failed, self.vr_enable, view);

        if let Some(frame) = self.eye_views {
            self.view = view_proj(&frame.views[0]);
        }

        if !self.mesh_ready || self.mesh_revision != revision {
            if vertices.is_empty() {
                self.mesh_vertices = 0;
                self.mesh_revision = revision;
                self.mesh_ready = true;
            } else if self.upload_mesh(vertices).is_ok() {
                self.mesh_vertices = (vertices.len() / 6) as u32;
                self.mesh_revision = revision;
                self.mesh_ready = true;
            }
        }

        self.draw_mesh = self.mesh_ready && self.mesh_vertices > 0;
    }

    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        push_rect(&mut self.ui, x, y, w, h, color.as_rgba_f32());
    }

    fn draw_outlined_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, thickness: f32, color: Color) {
        push_outline(&mut self.ui, x, y, w, h, thickness, color.as_rgba_f32());
    }

    fn draw_text(&mut self, _font: &str, text: &str, x: f32, y: f32, scale: f32, color: Color) {
        self.text.queue(text, x, y, scale, color.as_rgba_f32());
    }

    fn render_text(&mut self) {
        self.text.build();
    }

    fn enable_vr(&mut self) {
        self.vr_enable = true;
    }

    fn vr_input(&self) -> VrInput {
        match self.vr.as_ref() {
            Some(headset) => headset.input(),
            None => VrInput::default(),
        }
    }

    fn present(&mut self) {
        if !self.ready() {
            return;
        }

        if self.sync_atlas().is_err() {
            return;
        }

        let ui = write_buffer(&self.device, self.ui_buf.take(), bytes_of(&self.ui), 24);
        let text = write_buffer(&self.device, self.text_buf.take(), bytes_of(&self.text.verts), 32);
        self.ui_buf = ui.ok();
        self.text_buf = text.ok();
        bind_sampler(&self.device);

        unsafe {
            if self.device.BeginScene().is_err() {
                return;
            }
        }

        self.render_eyes();

        unsafe {
            let _ = self.device.SetViewport(&D3DVIEWPORT9 {
                X: 0,
                Y: 0,
                Width: self.width,
                Height: self.height,
                MinZ: 0.0,
                MaxZ: 1.0,
            });
            let _ = self.device.Clear(0, std::ptr::null(), (D3DCLEAR_TARGET | D3DCLEAR_ZBUFFER) as u32, pack_color(self.clear[0], self.clear[1], self.clear[2]), 1.0, 0);
        }

        if self.draw_mesh {
            if let Some(mesh) = self.mesh.as_ref() {
                self.draw_buffer(&self.mesh_vs, &self.mesh_ps, &self.mesh_decl, &self.view, mesh, self.mesh_vertices, true, false, None);
            }
        }

        let mut screen = [0.0; 4];
        screen[0] = self.width as f32;
        screen[1] = self.height as f32;

        if let Some(ui) = self.ui_buf.as_ref() {
            self.draw_buffer(&self.color_vs, &self.color_ps, &self.color_decl, &screen, ui, (self.ui.len() / 6) as u32, false, true, None);
        }

        if let Some(text) = self.text_buf.as_ref() {
            self.draw_buffer(
                &self.text_vs,
                &self.text_ps,
                &self.text_decl,
                &screen,
                text,
                (self.text.verts.len() / 8) as u32,
                false,
                true,
                self.atlas.as_ref(),
            );
        }

        unsafe {
            let _ = self.device.EndScene();
        }

        self.submit_eyes();

        unsafe {
            let _ = self.device.Present(std::ptr::null(), std::ptr::null(), HWND::default(), std::ptr::null());
        }

        if let Some(headset) = self.vr.as_mut() {
            headset.handoff();
        }
    }
}

impl D3D9Window {
    fn render_eyes(&mut self) {
        let Some(frame) = self.eye_views else {
            return;
        };

        if self.ensure_eyes(frame.width, frame.height).is_err() {
            return;
        }

        let previous = unsafe { self.device.GetRenderTarget(0).ok() };
        let previous_depth = unsafe { self.device.GetDepthStencilSurface().ok() };
        let Some(previous) = previous else {
            return;
        };
        let (surface, depth, width, height) = {
            let Some(eyes) = &self.eyes else {
                return;
            };

            (
                [eyes.surface[0].clone(), eyes.surface[1].clone()],
                [eyes.depth[0].clone(), eyes.depth[1].clone()],
                eyes.width,
                eyes.height,
            )
        };
        let mut idx = 0;

        while idx < 2 {
            let matrix = view_proj(&frame.views[idx]);
            unsafe {
                let _ = self.device.SetRenderTarget(0, &surface[idx]);
                let _ = self.device.SetDepthStencilSurface(&depth[idx]);
                let _ = self.device.SetViewport(&D3DVIEWPORT9 {
                    X: 0,
                    Y: 0,
                    Width: width,
                    Height: height,
                    MinZ: 0.0,
                    MaxZ: 1.0,
                });
                let _ = self.device.Clear(0, std::ptr::null(), (D3DCLEAR_TARGET | D3DCLEAR_ZBUFFER) as u32, pack_color(self.clear[0], self.clear[1], self.clear[2]), 1.0, 0);
            }

            if self.draw_mesh {
                if let Some(mesh) = self.mesh.as_ref() {
                    self.draw_buffer(&self.mesh_vs, &self.mesh_ps, &self.mesh_decl, &matrix, mesh, self.mesh_vertices, true, false, None);
                }
            }

            idx += 1;
        }

        unsafe {
            let _ = self.device.SetRenderTarget(0, &previous);

            if let Some(depth) = previous_depth.as_ref() {
                let _ = self.device.SetDepthStencilSurface(depth);
            }
        }

    }

    fn submit_eyes(&mut self) {
        if self.eye_views.is_none() {
            return;
        }

        let shared = {
            let Some(eyes) = &self.eyes else {
                return;
            };

            eyes.shared
        };
        let Some(headset) = self.vr.as_mut() else {
            return;
        };
        headset.submit_shared(0, shared[0].0);
        headset.submit_shared(1, shared[1].0);
    }

    fn ensure_eyes(&mut self, width: u32, height: u32) -> Result<(), String> {
        let width = width.max(1);
        let height = height.max(1);

        if let Some(eyes) = &self.eyes {
            if eyes.width == width && eyes.height == height {
                return Ok(());
            }
        }

        let mut color = Vec::new();
        let mut surface = Vec::new();
        let mut shared = Vec::new();
        let mut depth = Vec::new();
        let mut idx = 0;

        while idx < 2 {
            let (texture, face, handle) = shared_color(&self.device, width, height)?;
            let stencil = eye_depth(&self.device, width, height)?;
            color.push(texture);
            surface.push(face);
            shared.push(handle);
            depth.push(stencil);
            idx += 1;
        }

        self.eyes = Some(Eyes9 {
            width,
            height,
            color: [color.remove(0), color.remove(0)],
            surface: [surface.remove(0), surface.remove(0)],
            shared: [shared.remove(0), shared.remove(0)],
            depth: [depth.remove(0), depth.remove(0)],
        });

        Ok(())
    }
}

impl Drop for D3D9Window {
    fn drop(&mut self) {
        self.vr.take();
    }
}

fn shared_color(device: &IDirect3DDevice9, width: u32, height: u32) -> Result<(IDirect3DTexture9, IDirect3DSurface9, HANDLE), String> {
    let mut shared = HANDLE(std::ptr::null_mut());
    let mut texture = None;
    unsafe {
        device
            .CreateTexture(width, height, 1, D3DUSAGE_RENDERTARGET as u32, D3DFMT_A8R8G8B8, D3DPOOL_DEFAULT, &mut texture, &mut shared)
            .map_err(|err| err.to_string())?;
    }
    let texture = texture.ok_or_else(|| "vr color".to_string())?;
    let surface = unsafe { texture.GetSurfaceLevel(0).map_err(|err| err.to_string())? };

    Ok((texture, surface, shared))
}

fn eye_depth(device: &IDirect3DDevice9, width: u32, height: u32) -> Result<IDirect3DSurface9, String> {
    let mut depth = None;
    unsafe {
        device
            .CreateDepthStencilSurface(width, height, D3DFMT_D24S8, D3DMULTISAMPLE_NONE, 0, true, &mut depth, std::ptr::null_mut())
            .map_err(|err| err.to_string())?;
    }

    depth.ok_or_else(|| "vr depth".to_string())
}

fn create_device(hwnd: HWND, width: u32, height: u32) -> Result<(IDirect3DDevice9, D3DPRESENT_PARAMETERS), String> {
    if let Ok(d3d) = unsafe { Direct3DCreate9Ex(D3D_SDK_VERSION) } {
        if let Ok(device) = create_device_ex(&d3d, hwnd, width, height) {
            return Ok(device);
        }
    }

    let d3d = unsafe { Direct3DCreate9(D3D_SDK_VERSION).ok_or_else(|| "d3d9".to_string())? };

    create_device_hal(&d3d, hwnd, width, height)
}

fn create_device_ex(d3d: &IDirect3D9Ex, hwnd: HWND, width: u32, height: u32) -> Result<(IDirect3DDevice9, D3DPRESENT_PARAMETERS), String> {
    let flags = [D3DCREATE_HARDWARE_VERTEXPROCESSING, D3DCREATE_SOFTWARE_VERTEXPROCESSING];
    let mut last = "d3d9ex device".to_string();

    for flag in flags {
        let mut params = present_params(hwnd, width, height);
        let mut device = None;
        let created = unsafe {
            d3d.CreateDeviceEx(
                D3DADAPTER_DEFAULT,
                D3DDEVTYPE_HAL,
                hwnd,
                flag as u32,
                &mut params,
                std::ptr::null_mut(),
                &mut device,
            )
        };

        if let Err(err) = created {
            last = err.to_string();

            continue;
        }

        if let Some(device) = device {
            let device = device.cast::<IDirect3DDevice9>().map_err(|err| err.to_string())?;

            return Ok((device, params));
        }
    }

    Err(last)
}

fn create_device_hal(d3d: &IDirect3D9, hwnd: HWND, width: u32, height: u32) -> Result<(IDirect3DDevice9, D3DPRESENT_PARAMETERS), String> {
    let flags = [D3DCREATE_HARDWARE_VERTEXPROCESSING, D3DCREATE_SOFTWARE_VERTEXPROCESSING];
    let mut last = "d3d9 device".to_string();

    for flag in flags {
        let mut params = present_params(hwnd, width, height);
        let mut device = None;
        let created = unsafe {
            d3d.CreateDevice(
                D3DADAPTER_DEFAULT,
                D3DDEVTYPE_HAL,
                hwnd,
                flag as u32,
                &mut params,
                &mut device,
            )
        };

        if let Err(err) = created {
            last = err.to_string();
            continue;
        }

        if let Some(device) = device {
            return Ok((device, params));
        }
    }

    Err(last)
}

fn present_params(hwnd: HWND, width: u32, height: u32) -> D3DPRESENT_PARAMETERS {
    D3DPRESENT_PARAMETERS {
        BackBufferWidth: width,
        BackBufferHeight: height,
        BackBufferFormat: D3DFMT_UNKNOWN,
        BackBufferCount: 1,
        MultiSampleType: D3DMULTISAMPLE_NONE,
        MultiSampleQuality: 0,
        SwapEffect: D3DSWAPEFFECT_DISCARD,
        hDeviceWindow: hwnd,
        Windowed: true.into(),
        EnableAutoDepthStencil: true.into(),
        AutoDepthStencilFormat: D3DFMT_D24S8,
        Flags: 0,
        FullScreen_RefreshRateInHz: 0,
        PresentationInterval: D3DPRESENT_INTERVAL_IMMEDIATE as u32,
    }
}

fn vertex_shader(device: &IDirect3DDevice9, blob: &windows::Win32::Graphics::Direct3D::ID3DBlob) -> Result<IDirect3DVertexShader9, String> {
    unsafe { device.CreateVertexShader(blob_bytes(blob).as_ptr() as *const u32).map_err(|err| err.to_string()) }
}

fn pixel_shader(device: &IDirect3DDevice9, blob: &windows::Win32::Graphics::Direct3D::ID3DBlob) -> Result<IDirect3DPixelShader9, String> {
    unsafe { device.CreatePixelShader(blob_bytes(blob).as_ptr() as *const u32).map_err(|err| err.to_string()) }
}

fn declaration(device: &IDirect3DDevice9, elements: &[D3DVERTEXELEMENT9]) -> Result<IDirect3DVertexDeclaration9, String> {
    unsafe { device.CreateVertexDeclaration(elements.as_ptr()).map_err(|err| err.to_string()) }
}

fn element(offset: u16, kind: D3DDECLTYPE, usage: D3DDECLUSAGE) -> D3DVERTEXELEMENT9 {
    D3DVERTEXELEMENT9 {
        Stream: 0,
        Offset: offset,
        Type: kind.0 as u8,
        Method: D3DDECLMETHOD_DEFAULT.0 as u8,
        Usage: usage.0 as u8,
        UsageIndex: 0,
    }
}

fn ended(mut elements: Vec<D3DVERTEXELEMENT9>) -> Vec<D3DVERTEXELEMENT9> {
    elements.push(D3DVERTEXELEMENT9 {
        Stream: 0xff,
        Offset: 0,
        Type: D3DDECLTYPE_UNUSED.0 as u8,
        Method: 0,
        Usage: 0,
        UsageIndex: 0,
    });

    elements
}

fn mesh_decl() -> Vec<D3DVERTEXELEMENT9> {
    ended(vec![
        element(0, D3DDECLTYPE_FLOAT3, D3DDECLUSAGE_POSITION),
        element(12, D3DDECLTYPE_FLOAT3, D3DDECLUSAGE_COLOR),
    ])
}

fn color_decl() -> Vec<D3DVERTEXELEMENT9> {
    ended(vec![
        element(0, D3DDECLTYPE_FLOAT2, D3DDECLUSAGE_POSITION),
        element(8, D3DDECLTYPE_FLOAT4, D3DDECLUSAGE_COLOR),
    ])
}

fn text_decl() -> Vec<D3DVERTEXELEMENT9> {
    ended(vec![
        element(0, D3DDECLTYPE_FLOAT2, D3DDECLUSAGE_POSITION),
        element(8, D3DDECLTYPE_FLOAT2, D3DDECLUSAGE_TEXCOORD),
        element(16, D3DDECLTYPE_FLOAT4, D3DDECLUSAGE_COLOR),
    ])
}

fn write_buffer(device: &IDirect3DDevice9, current: Option<GpuBuf>, bytes: &[u8], stride: u32) -> Result<GpuBuf, String> {
    let mut buffer = match current {
        Some(buffer) if buffer.capacity >= bytes.len() as u32 && buffer.stride == stride => buffer,
        _ => {
            let capacity = grow(0, bytes.len().max(1) as u32);
            GpuBuf {
                buffer: vertex_buffer(device, capacity)?,
                capacity,
                stride,
            }
        }
    };

    if bytes.is_empty() {
        return Ok(buffer);
    }

    if buffer.capacity < bytes.len() as u32 {
        buffer.capacity = grow(buffer.capacity, bytes.len() as u32);
        buffer.buffer = vertex_buffer(device, buffer.capacity)?;
    }

    unsafe {
        let mut ptr = std::ptr::null_mut();
        buffer.buffer.Lock(0, bytes.len() as u32, &mut ptr, 0).map_err(|err| err.to_string())?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        buffer.buffer.Unlock().map_err(|err| err.to_string())?;
    }

    Ok(buffer)
}

fn vertex_buffer(device: &IDirect3DDevice9, bytes: u32) -> Result<IDirect3DVertexBuffer9, String> {
    let mut buffer = None;
    unsafe {
        device
            .CreateVertexBuffer(bytes.max(4), 0, 0, D3DPOOL_MANAGED, &mut buffer, std::ptr::null_mut())
            .map_err(|err| err.to_string())?;
    }

    buffer.ok_or_else(|| "vertex buffer".to_string())
}

fn atlas_texture(device: &IDirect3DDevice9, width: u32, height: u32) -> Result<IDirect3DTexture9, String> {
    let mut texture = None;
    unsafe {
        device
            .CreateTexture(width.max(1), height.max(1), 1, 0, D3DFMT_A8R8G8B8, D3DPOOL_MANAGED, &mut texture, std::ptr::null_mut())
            .map_err(|err| err.to_string())?;
    }

    texture.ok_or_else(|| "atlas".to_string())
}

fn bind_sampler(device: &IDirect3DDevice9) {
    unsafe {
        let _ = device.SetSamplerState(0, D3DSAMP_MINFILTER, D3DTEXF_LINEAR.0 as u32);
        let _ = device.SetSamplerState(0, D3DSAMP_MAGFILTER, D3DTEXF_LINEAR.0 as u32);
        let _ = device.SetSamplerState(0, D3DSAMP_MIPFILTER, D3DTEXF_NONE.0 as u32);
        let _ = device.SetSamplerState(0, D3DSAMP_ADDRESSU, D3DTADDRESS_CLAMP.0 as u32);
        let _ = device.SetSamplerState(0, D3DSAMP_ADDRESSV, D3DTADDRESS_CLAMP.0 as u32);
    }
}

fn expand_bgra(src: &[u8], width: u32, height: u32, dst: *mut u8, pitch: i32) {
    if dst.is_null() || pitch <= 0 {
        return;
    }

    let mut row = 0;

    while row < height {
        let src_row = (row * width) as usize;
        let dst_row = unsafe { dst.add((row as i32 * pitch) as usize) };
        let mut col = 0;

        while col < width {
            let coverage = src[src_row + col as usize];
            unsafe {
                let pixel = dst_row.add(col as usize * 4);
                *pixel = coverage;
                *pixel.add(1) = coverage;
                *pixel.add(2) = coverage;
                *pixel.add(3) = 255;
            }
            col += 1;
        }

        row += 1;
    }
}

fn pack_color(red: f32, green: f32, blue: f32) -> u32 {
    let r = (red.clamp(0.0, 1.0) * 255.0) as u32;
    let g = (green.clamp(0.0, 1.0) * 255.0) as u32;
    let b = (blue.clamp(0.0, 1.0) * 255.0) as u32;

    (255 << 24) | (r << 16) | (g << 8) | b
}
