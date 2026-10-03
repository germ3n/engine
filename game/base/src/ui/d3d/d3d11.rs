use crate::platform::Surface;
use crate::ui::batch::{bytes_of, grow, push_outline, push_rect, TextFrame};
use crate::ui::d3d::draw::{attach_desktop, Desktop};
use crate::ui::d3d::math::view_proj;
use crate::ui::d3d::shader::{self, blob_bytes};
use crate::ui::shader as shaders;
use crate::ui::voxel::SceneView;
use crate::ui::vr::{self, EyeViews, Headset, VrInput};
use crate::ui::window::Window;
use crate::ui::Color;
use windows::core::{s, Interface};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

pub struct D3D11Window {
    desktop: Desktop,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swap: IDXGISwapChain,
    rtv: Option<ID3D11RenderTargetView>,
    depth: Option<ID3D11Texture2D>,
    dsv: Option<ID3D11DepthStencilView>,
    mesh_vs: ID3D11VertexShader,
    mesh_ps: ID3D11PixelShader,
    mesh_layout: ID3D11InputLayout,
    color_vs: ID3D11VertexShader,
    color_ps: ID3D11PixelShader,
    color_layout: ID3D11InputLayout,
    text_vs: ID3D11VertexShader,
    text_ps: ID3D11PixelShader,
    text_layout: ID3D11InputLayout,
    view_cb: ID3D11Buffer,
    screen_cb: ID3D11Buffer,
    mesh: Option<DynBuf>,
    ui_buf: Option<DynBuf>,
    text_buf: Option<DynBuf>,
    atlas: Option<ID3D11Texture2D>,
    atlas_view: Option<ID3D11ShaderResourceView>,
    atlas_size: (u32, u32),
    sampler: ID3D11SamplerState,
    depth_on: ID3D11DepthStencilState,
    depth_off: ID3D11DepthStencilState,
    blend_off: ID3D11BlendState,
    blend_on: ID3D11BlendState,
    cull_back: ID3D11RasterizerState,
    cull_none: ID3D11RasterizerState,
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
    user: Vec<DxUser>,
    user_cb: Option<ID3D11Buffer>,
    bound_rtv: Option<ID3D11RenderTargetView>,
    bound_dsv: Option<ID3D11DepthStencilView>,
    text_once: Option<crate::ui::batch::TextFrame>,
    vr: Option<Headset>,
    vr_failed: bool,
    vr_enable: bool,
    eyes: Option<Eyes11>,
    eye_views: Option<EyeViews>,
}

struct Eyes11 {
    width: u32,
    height: u32,
    color: [ID3D11Texture2D; 2],
    rtv: [ID3D11RenderTargetView; 2],
    depth: [ID3D11Texture2D; 2],
    dsv: [ID3D11DepthStencilView; 2],
}

#[derive(Clone)]
struct DynBuf {
    buffer: ID3D11Buffer,
    capacity: u32,
    stride: u32,
}

impl D3D11Window {
    pub fn try_new(surface: &Surface) -> Result<Self, String> {
        let desktop = attach_desktop(surface)?;
        let width = desktop.width;
        let height = desktop.height;
        let (device, context, swap) = device_swap(desktop.hwnd, width, height)?;
        let (rtv, depth, dsv) = targets(&device, &swap, width, height)?;
        let cache = adapter_cache(&device)?;
        let mesh_src = cache.hlsl(&crate::ui::shaders::Program::Mesh.wgsl())?;
        let color_src = cache.hlsl(&crate::ui::shaders::Program::Color.wgsl())?;
        let text_src = cache.hlsl(&crate::ui::shaders::Program::Text.wgsl())?;
        let mesh_vs_blob = shader::vs5(&mesh_src, s!("vs_main"))?;
        let mesh_ps_blob = shader::ps5(&mesh_src, s!("fs_main"))?;
        let color_vs_blob = shader::vs5(&color_src, s!("vs_main"))?;
        let color_ps_blob = shader::ps5(&color_src, s!("fs_main"))?;
        let text_vs_blob = shader::vs5(&text_src, s!("vs_main"))?;
        let text_ps_blob = shader::ps5(&text_src, s!("fs_main"))?;
        let mesh_vs = vertex_shader(&device, &mesh_vs_blob)?;
        let mesh_ps = pixel_shader(&device, &mesh_ps_blob)?;
        let color_vs = vertex_shader(&device, &color_vs_blob)?;
        let color_ps = pixel_shader(&device, &color_ps_blob)?;
        let text_vs = vertex_shader(&device, &text_vs_blob)?;
        let text_ps = pixel_shader(&device, &text_ps_blob)?;
        let mesh_layout = input_layout(&device, &mesh_vs_blob, &mesh_elements())?;
        let color_layout = input_layout(&device, &color_vs_blob, &color_elements())?;
        let text_layout = input_layout(&device, &text_vs_blob, &text_elements())?;
        let view_cb = constant_buffer(&device, 64)?;
        let screen_cb = constant_buffer(&device, 64)?;
        let sampler = sampler_state(&device)?;
        let depth_on = depth_state(&device, true)?;
        let depth_off = depth_state(&device, false)?;
        let blend_off = blend_state(&device, false)?;
        let blend_on = blend_state(&device, true)?;
        let cull_back = rasterizer(&device, D3D11_CULL_BACK)?;
        let cull_none = rasterizer(&device, D3D11_CULL_NONE)?;
        let text = TextFrame::new()?;

        Ok(Self {
            desktop,
            device,
            context,
            swap,
            rtv: Some(rtv),
            depth: Some(depth),
            dsv: Some(dsv),
            mesh_vs,
            mesh_ps,
            mesh_layout,
            color_vs,
            color_ps,
            color_layout,
            text_vs,
            text_ps,
            text_layout,
            view_cb,
            screen_cb,
            mesh: None,
            ui_buf: None,
            text_buf: None,
            atlas: None,
            atlas_view: None,
            atlas_size: (0, 0),
            sampler,
            depth_on,
            depth_off,
            blend_off,
            blend_on,
            cull_back,
            cull_none,
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
            user: Vec::new(),
            user_cb: None,
            bound_rtv: None,
            bound_dsv: None,
            text_once: None,
            vr: None,
            vr_failed: false,
            vr_enable: false,
            eyes: None,
            eye_views: None,
        })
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        let width = width.max(1);
        let height = height.max(1);

        if self.width == width && self.height == height && self.rtv.is_some() {
            return Ok(());
        }

        unsafe {
            self.context
                .OMSetRenderTargets(None, None::<&ID3D11DepthStencilView>);
            self.context.Flush();
        }

        self.rtv = None;
        self.dsv = None;
        self.depth = None;
        unsafe {
            self.swap
                .ResizeBuffers(
                    0,
                    width,
                    height,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
                .map_err(|err| err.to_string())?;
        }
        let (rtv, depth, dsv) = targets(&self.device, &self.swap, width, height)?;
        self.rtv = Some(rtv);
        self.depth = Some(depth);
        self.dsv = Some(dsv);
        self.width = width;
        self.height = height;

        Ok(())
    }

    fn upload_mesh(&mut self, vertices: &[f32]) -> Result<(), String> {
        let bytes = bytes_of(vertices);
        let buffer = self.mesh.take();
        self.mesh = Some(write_dynamic(
            &self.device,
            &self.context,
            buffer,
            bytes,
            (crate::world::STRIDE * 4) as u32,
        )?);

        Ok(())
    }

    fn sync_atlas(&mut self) -> Result<(), String> {
        if !self.text.dirty && self.atlas_size == self.text.size && self.atlas_view.is_some() {
            return Ok(());
        }

        if self.atlas_size != self.text.size || self.atlas.is_none() {
            let texture = atlas_texture(&self.device, self.text.size.0, self.text.size.1)?;
            let view = atlas_view(&self.device, &texture)?;
            self.atlas = Some(texture);
            self.atlas_view = Some(view);
            self.atlas_size = self.text.size;
        }

        if let Some(texture) = self.atlas.as_ref() {
            let pitch = self.text.size.0;
            unsafe {
                self.context.UpdateSubresource(
                    texture,
                    0,
                    None,
                    self.text.pixels.as_ptr() as *const _,
                    pitch,
                    pitch * self.text.size.1,
                );
            }
        }

        self.text.dirty = false;

        Ok(())
    }
}

impl Window for D3D11Window {
    fn attach(surface: &Surface) -> Self {
        Self::try_new(surface).expect("d3d11")
    }

    fn set_size(&mut self, w: u32, h: u32) {
        if let Err(err) = self.resize(w, h) {
            log::warn!("[gfx] d3d11 resize {err}");
        }
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        self.clear = [red, green, blue, 1.0];
        self.ui.clear();
        self.user.clear();
        self.bound_rtv = None;
        self.bound_dsv = None;
        self.draw_mesh = false;
    }

    fn draw_colored_mesh(
        &mut self,
        vertices: &[f32],
        ranges: &[crate::world::SurfaceRange],
        graphics: &crate::world::MapGraphics,
        revision: u64,
        view: &SceneView,
    ) {
        let _ = (ranges, graphics);
        self.view = view_proj(view);
        self.eye_views = vr::connect(&mut self.vr, &mut self.vr_failed, self.vr_enable, view);

        if let Some(eyes) = self.eye_views {
            self.view = view_proj(&eyes.views[0]);
        }

        if !self.mesh_ready || self.mesh_revision != revision {
            if vertices.is_empty() {
                self.mesh_vertices = 0;
                self.mesh_revision = revision;
                self.mesh_ready = true;
            } else if self.upload_mesh(vertices).is_ok() {
                self.mesh_vertices = (vertices.len() / crate::world::STRIDE) as u32;
                self.mesh_revision = revision;
                self.mesh_ready = true;
            }
        }

        self.draw_mesh = self.mesh_ready && self.mesh_vertices > 0;
    }

    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        push_rect(&mut self.ui, x, y, w, h, color.as_rgba_f32());
    }

    fn draw_outlined_rectangle(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        thickness: f32,
        color: Color,
    ) {
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
        if self.width == 0 || self.height == 0 {
            return;
        }

        self.render_eyes();

        let Some(rtv) = self.rtv.clone() else {
            return;
        };
        let Some(dsv) = self.dsv.clone() else {
            return;
        };

        if self.sync_atlas().is_err() {
            return;
        }

        let ui = write_dynamic(
            &self.device,
            &self.context,
            self.ui_buf.take(),
            bytes_of(&self.ui),
            24,
        );
        let text = write_dynamic(
            &self.device,
            &self.context,
            self.text_buf.take(),
            bytes_of(&self.text.verts),
            32,
        );
        self.ui_buf = ui.ok();
        self.text_buf = text.ok();
        let _ = write_constants(&self.context, &self.view_cb, &self.view);
        let mut screen = [0.0; 16];
        screen[0] = self.width as f32;
        screen[1] = self.height as f32;
        let _ = write_constants(&self.context, &self.screen_cb, &screen);

        unsafe {
            self.context.OMSetRenderTargets(Some(&[Some(rtv)]), &dsv);
            self.context
                .ClearRenderTargetView(self.rtv.as_ref().unwrap(), &self.clear);
            self.context.ClearDepthStencilView(
                self.dsv.as_ref().unwrap(),
                D3D11_CLEAR_DEPTH.0,
                1.0,
                0,
            );
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            self.context
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        }

        if self.draw_mesh {
            if let Some(mesh) = self.mesh.as_ref() {
                self.draw_buffer(
                    &self.mesh_vs,
                    &self.mesh_ps,
                    &self.mesh_layout,
                    &self.view_cb,
                    mesh,
                    self.mesh_vertices,
                    &self.depth_on,
                    &self.blend_off,
                    &self.cull_back,
                    None,
                    None,
                );
            }
        }

        if let Some(ui) = self.ui_buf.as_ref() {
            let count = (self.ui.len() / 6) as u32;
            self.draw_buffer(
                &self.color_vs,
                &self.color_ps,
                &self.color_layout,
                &self.screen_cb,
                ui,
                count,
                &self.depth_off,
                &self.blend_on,
                &self.cull_none,
                None,
                None,
            );
        }

        if let Some(text) = self.text_buf.as_ref() {
            let count = (self.text.verts.len() / 8) as u32;
            self.draw_buffer(
                &self.text_vs,
                &self.text_ps,
                &self.text_layout,
                &self.screen_cb,
                text,
                count,
                &self.depth_off,
                &self.blend_on,
                &self.cull_none,
                self.atlas_view.clone(),
                None,
            );
        }

        self.flush_user();
        let _ = unsafe { self.swap.Present(0, DXGI_PRESENT(0)) };

        if let Some(headset) = self.vr.as_mut() {
            headset.handoff();
        }
    }
}

impl D3D11Window {
    fn render_eyes(&mut self) {
        let Some(frame) = self.eye_views else {
            return;
        };

        if self.ensure_eyes(frame.width, frame.height).is_err() {
            return;
        }

        let (color, rtv, dsv, width, height) = {
            let Some(eyes) = &self.eyes else {
                return;
            };

            (
                [eyes.color[0].clone(), eyes.color[1].clone()],
                [eyes.rtv[0].clone(), eyes.rtv[1].clone()],
                [eyes.dsv[0].clone(), eyes.dsv[1].clone()],
                eyes.width,
                eyes.height,
            )
        };
        let mesh = self.mesh.clone();
        let mut idx = 0;

        while idx < 2 {
            let matrix = view_proj(&frame.views[idx]);
            let _ = write_constants(&self.context, &self.view_cb, &matrix);
            unsafe {
                self.context
                    .OMSetRenderTargets(Some(&[Some(rtv[idx].clone())]), &dsv[idx]);
                self.context.ClearRenderTargetView(&rtv[idx], &self.clear);
                self.context
                    .ClearDepthStencilView(&dsv[idx], D3D11_CLEAR_DEPTH.0, 1.0, 0);
                self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: width as f32,
                    Height: height as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                }]));
                self.context
                    .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            }

            if self.draw_mesh {
                if let Some(buffer) = mesh.as_ref() {
                    self.draw_buffer(
                        &self.mesh_vs,
                        &self.mesh_ps,
                        &self.mesh_layout,
                        &self.view_cb,
                        buffer,
                        self.mesh_vertices,
                        &self.depth_on,
                        &self.blend_off,
                        &self.cull_back,
                        None,
                        None,
                    );
                }
            }

            idx += 1;
        }

        unsafe {
            self.context.Flush();
        }

        if let Some(headset) = self.vr.as_mut() {
            headset.submit_d3d11(0, color[0].as_raw());
            headset.submit_d3d11(1, color[1].as_raw());
        }
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
        let mut rtv = Vec::new();
        let mut depth = Vec::new();
        let mut dsv = Vec::new();
        let mut idx = 0;

        while idx < 2 {
            let (texture, view) = eye_color(&self.device, width, height)?;
            let (depth_tex, depth_view) = eye_depth(&self.device, width, height)?;
            color.push(texture);
            rtv.push(view);
            depth.push(depth_tex);
            dsv.push(depth_view);
            idx += 1;
        }

        self.eyes = Some(Eyes11 {
            width,
            height,
            color: [color.remove(0), color.remove(0)],
            rtv: [rtv.remove(0), rtv.remove(0)],
            depth: [depth.remove(0), depth.remove(0)],
            dsv: [dsv.remove(0), dsv.remove(0)],
        });

        Ok(())
    }

    fn draw_buffer(
        &self,
        vs: &ID3D11VertexShader,
        ps: &ID3D11PixelShader,
        layout: &ID3D11InputLayout,
        constants: &ID3D11Buffer,
        buffer: &DynBuf,
        vertices: u32,
        depth: &ID3D11DepthStencilState,
        blend: &ID3D11BlendState,
        raster: &ID3D11RasterizerState,
        texture: Option<ID3D11ShaderResourceView>,
        sampler: Option<ID3D11SamplerState>,
    ) {
        if vertices == 0 {
            return;
        }

        let buffers = [Some(buffer.buffer.clone())];
        let stride = buffer.stride;
        let offset = 0u32;

        unsafe {
            self.context.IASetInputLayout(layout);
            self.context.IASetVertexBuffers(
                0,
                1,
                Some(buffers.as_ptr()),
                Some(&stride),
                Some(&offset),
            );
            self.context.VSSetShader(vs, None);
            self.context.PSSetShader(ps, None);
            self.context
                .VSSetConstantBuffers(0, Some(&[Some(constants.clone())]));
            self.context.OMSetDepthStencilState(depth, 0);
            self.context
                .OMSetBlendState(blend, Some(&[0.0, 0.0, 0.0, 0.0]), u32::MAX);
            self.context.RSSetState(raster);

            if let Some(texture) = texture {
                self.context.PSSetShaderResources(0, Some(&[Some(texture)]));
                let sampler = sampler.unwrap_or_else(|| self.sampler.clone());
                self.context.PSSetSamplers(0, Some(&[Some(sampler)]));
            }

            self.context.Draw(vertices, 0);
        }
    }
}

fn adapter_cache(device: &ID3D11Device) -> Result<shaders::Registry, String> {
    let dxgi: IDXGIDevice = device.cast().map_err(|err| err.to_string())?;
    let adapter: IDXGIAdapter = unsafe { dxgi.GetAdapter().map_err(|err| err.to_string())? };
    let desc = unsafe { adapter.GetDesc().map_err(|err| err.to_string())? };

    Ok(shaders::Registry::for_device(&shaders::id_from_luid(
        desc.AdapterLuid.LowPart,
        desc.AdapterLuid.HighPart,
    )))
}

fn device_swap(
    hwnd: HWND,
    width: u32,
    height: u32,
) -> Result<(ID3D11Device, ID3D11DeviceContext, IDXGISwapChain), String> {
    let levels = [
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];
    let drivers = [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP];
    let mut last = "d3d11 device".to_string();

    for driver in drivers {
        let mut swap = None;
        let mut device = None;
        let mut context = None;
        let mut feature = D3D_FEATURE_LEVEL_11_0;
        let desc = swap_desc(hwnd, width, height);
        let created = unsafe {
            D3D11CreateDeviceAndSwapChain(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&desc),
                Some(&mut swap),
                Some(&mut device),
                Some(&mut feature),
                Some(&mut context),
            )
        };

        if let Err(err) = created {
            last = err.to_string();
            continue;
        }

        if let (Some(device), Some(context), Some(swap)) = (device, context, swap) {
            return Ok((device, context, swap));
        }
    }

    Err(last)
}

fn swap_desc(hwnd: HWND, width: u32, height: u32) -> DXGI_SWAP_CHAIN_DESC {
    DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: width,
            Height: height,
            RefreshRate: DXGI_RATIONAL {
                Numerator: 0,
                Denominator: 1,
            },
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
            Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 1,
        OutputWindow: hwnd,
        Windowed: true.into(),
        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
        Flags: 0,
    }
}

impl Drop for D3D11Window {
    fn drop(&mut self) {
        self.vr.take();
    }
}

fn eye_color(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, ID3D11RenderTargetView), String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
    };
    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .map_err(|err| err.to_string())?;
    }
    let texture = texture.ok_or_else(|| "vr color".to_string())?;
    let mut view = None;
    unsafe {
        device
            .CreateRenderTargetView(&texture, None, Some(&mut view))
            .map_err(|err| err.to_string())?;
    }
    let view = view.ok_or_else(|| "vr color view".to_string())?;

    Ok((texture, view))
}

fn eye_depth(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, ID3D11DepthStencilView), String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_D32_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .map_err(|err| err.to_string())?;
    }
    let texture = texture.ok_or_else(|| "vr depth".to_string())?;
    let mut view = None;
    unsafe {
        device
            .CreateDepthStencilView(&texture, None, Some(&mut view))
            .map_err(|err| err.to_string())?;
    }
    let view = view.ok_or_else(|| "vr depth view".to_string())?;

    Ok((texture, view))
}

fn targets(
    device: &ID3D11Device,
    swap: &IDXGISwapChain,
    width: u32,
    height: u32,
) -> Result<
    (
        ID3D11RenderTargetView,
        ID3D11Texture2D,
        ID3D11DepthStencilView,
    ),
    String,
> {
    let back: ID3D11Texture2D = unsafe { swap.GetBuffer(0).map_err(|err| err.to_string())? };
    let mut rtv = None;
    unsafe {
        device
            .CreateRenderTargetView(&back, None, Some(&mut rtv))
            .map_err(|err| err.to_string())?;
    }
    let rtv = rtv.ok_or_else(|| "render target".to_string())?;
    let mut depth = None;
    let depth_desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_D32_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    unsafe {
        device
            .CreateTexture2D(&depth_desc, None, Some(&mut depth))
            .map_err(|err| err.to_string())?;
    }
    let depth = depth.ok_or_else(|| "depth".to_string())?;
    let mut dsv = None;
    unsafe {
        device
            .CreateDepthStencilView(&depth, None, Some(&mut dsv))
            .map_err(|err| err.to_string())?;
    }
    let dsv = dsv.ok_or_else(|| "depth view".to_string())?;

    Ok((rtv, depth, dsv))
}

fn vertex_shader(
    device: &ID3D11Device,
    blob: &windows::Win32::Graphics::Direct3D::ID3DBlob,
) -> Result<ID3D11VertexShader, String> {
    let mut shader = None;
    unsafe {
        device
            .CreateVertexShader(
                blob_bytes(blob),
                None::<&ID3D11ClassLinkage>,
                Some(&mut shader),
            )
            .map_err(|err| err.to_string())?;
    }

    shader.ok_or_else(|| "vertex shader".to_string())
}

fn pixel_shader(
    device: &ID3D11Device,
    blob: &windows::Win32::Graphics::Direct3D::ID3DBlob,
) -> Result<ID3D11PixelShader, String> {
    let mut shader = None;
    unsafe {
        device
            .CreatePixelShader(
                blob_bytes(blob),
                None::<&ID3D11ClassLinkage>,
                Some(&mut shader),
            )
            .map_err(|err| err.to_string())?;
    }

    shader.ok_or_else(|| "pixel shader".to_string())
}

fn input_layout(
    device: &ID3D11Device,
    blob: &windows::Win32::Graphics::Direct3D::ID3DBlob,
    elements: &[D3D11_INPUT_ELEMENT_DESC],
) -> Result<ID3D11InputLayout, String> {
    let mut layout = None;
    unsafe {
        device
            .CreateInputLayout(elements, blob_bytes(blob), Some(&mut layout))
            .map_err(|err| err.to_string())?;
    }

    layout.ok_or_else(|| "input layout".to_string())
}

fn element(
    name: windows::core::PCSTR,
    index: u32,
    format: DXGI_FORMAT,
    offset: u32,
) -> D3D11_INPUT_ELEMENT_DESC {
    D3D11_INPUT_ELEMENT_DESC {
        SemanticName: name,
        SemanticIndex: index,
        Format: format,
        InputSlot: 0,
        AlignedByteOffset: offset,
        InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
        InstanceDataStepRate: 0,
    }
}

fn mesh_elements() -> [D3D11_INPUT_ELEMENT_DESC; 8] {
    [
        element(s!("LOC"), 0, DXGI_FORMAT_R32G32B32_FLOAT, 0),
        element(s!("LOC"), 1, DXGI_FORMAT_R32G32B32_FLOAT, 12),
        element(s!("LOC"), 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 24),
        element(s!("LOC"), 3, DXGI_FORMAT_R32G32_FLOAT, 40),
        element(s!("LOC"), 4, DXGI_FORMAT_R32G32_FLOAT, 48),
        element(s!("LOC"), 5, DXGI_FORMAT_R32G32B32_FLOAT, 56),
        element(s!("LOC"), 6, DXGI_FORMAT_R32_FLOAT, 68),
        element(s!("LOC"), 7, DXGI_FORMAT_R32_FLOAT, 72),
    ]
}

fn color_elements() -> [D3D11_INPUT_ELEMENT_DESC; 2] {
    [
        element(s!("LOC"), 0, DXGI_FORMAT_R32G32_FLOAT, 0),
        element(s!("LOC"), 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 8),
    ]
}

fn text_elements() -> [D3D11_INPUT_ELEMENT_DESC; 3] {
    [
        element(s!("LOC"), 0, DXGI_FORMAT_R32G32_FLOAT, 0),
        element(s!("LOC"), 1, DXGI_FORMAT_R32G32_FLOAT, 8),
        element(s!("LOC"), 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 16),
    ]
}

fn constant_buffer(device: &ID3D11Device, bytes: u32) -> Result<ID3D11Buffer, String> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: bytes,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe {
        device
            .CreateBuffer(&desc, None, Some(&mut buffer))
            .map_err(|err| err.to_string())?;
    }

    buffer.ok_or_else(|| "constant buffer".to_string())
}

fn vertex_buffer(device: &ID3D11Device, bytes: u32) -> Result<ID3D11Buffer, String> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: bytes.max(4),
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe {
        device
            .CreateBuffer(&desc, None, Some(&mut buffer))
            .map_err(|err| err.to_string())?;
    }

    buffer.ok_or_else(|| "vertex buffer".to_string())
}

fn write_dynamic(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    current: Option<DynBuf>,
    bytes: &[u8],
    stride: u32,
) -> Result<DynBuf, String> {
    let mut buffer = match current {
        Some(buffer) if buffer.capacity >= bytes.len() as u32 && buffer.stride == stride => buffer,
        _ => DynBuf {
            buffer: vertex_buffer(device, grow(0, bytes.len() as u32))?,
            capacity: grow(0, bytes.len().max(1) as u32),
            stride,
        },
    };

    if bytes.is_empty() {
        return Ok(buffer);
    }

    if buffer.capacity < bytes.len() as u32 {
        let capacity = grow(buffer.capacity, bytes.len() as u32);
        buffer.buffer = vertex_buffer(device, capacity)?;
        buffer.capacity = capacity;
    }

    unsafe {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(
                &buffer.buffer,
                0,
                D3D11_MAP_WRITE_DISCARD,
                0,
                Some(&mut mapped),
            )
            .map_err(|err| err.to_string())?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.pData as *mut u8, bytes.len());
        context.Unmap(&buffer.buffer, 0);
    }

    Ok(buffer)
}

fn write_constants(
    context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    values: &[f32; 16],
) -> Result<(), String> {
    let bytes = bytes_of(values);
    unsafe {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
            .map_err(|err| err.to_string())?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.pData as *mut u8, bytes.len());
        context.Unmap(buffer, 0);
    }

    Ok(())
}

fn atlas_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width.max(1),
        Height: height.max(1),
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .map_err(|err| err.to_string())?;
    }

    texture.ok_or_else(|| "atlas".to_string())
}

fn atlas_view(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
) -> Result<ID3D11ShaderResourceView, String> {
    let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    desc.Format = DXGI_FORMAT_R8_UNORM;
    desc.ViewDimension = D3D_SRV_DIMENSION_TEXTURE2D;
    desc.Anonymous.Texture2D = D3D11_TEX2D_SRV {
        MostDetailedMip: 0,
        MipLevels: 1,
    };
    let mut view = None;
    unsafe {
        device
            .CreateShaderResourceView(texture, Some(&desc), Some(&mut view))
            .map_err(|err| err.to_string())?;
    }

    view.ok_or_else(|| "atlas view".to_string())
}

fn sampler_state(device: &ID3D11Device) -> Result<ID3D11SamplerState, String> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MipLODBias: 0.0,
        MaxAnisotropy: 1,
        ComparisonFunc: D3D11_COMPARISON_NEVER,
        BorderColor: [0.0; 4],
        MinLOD: 0.0,
        MaxLOD: f32::MAX,
    };
    let mut sampler = None;
    unsafe {
        device
            .CreateSamplerState(&desc, Some(&mut sampler))
            .map_err(|err| err.to_string())?;
    }

    sampler.ok_or_else(|| "sampler".to_string())
}

fn depth_state(device: &ID3D11Device, enabled: bool) -> Result<ID3D11DepthStencilState, String> {
    let op = D3D11_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D11_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
        StencilPassOp: D3D11_STENCIL_OP_KEEP,
        StencilFunc: D3D11_COMPARISON_ALWAYS,
    };
    let desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: enabled.into(),
        DepthWriteMask: if enabled {
            D3D11_DEPTH_WRITE_MASK_ALL
        } else {
            D3D11_DEPTH_WRITE_MASK_ZERO
        },
        DepthFunc: if enabled {
            D3D11_COMPARISON_LESS
        } else {
            D3D11_COMPARISON_ALWAYS
        },
        StencilEnable: false.into(),
        StencilReadMask: 0,
        StencilWriteMask: 0,
        FrontFace: op,
        BackFace: op,
    };
    let mut state = None;
    unsafe {
        device
            .CreateDepthStencilState(&desc, Some(&mut state))
            .map_err(|err| err.to_string())?;
    }

    state.ok_or_else(|| "depth state".to_string())
}

fn blend_state(device: &ID3D11Device, enabled: bool) -> Result<ID3D11BlendState, String> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: enabled.into(),
        SrcBlend: D3D11_BLEND_SRC_ALPHA,
        DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ONE,
        DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    let mut state = None;
    unsafe {
        device
            .CreateBlendState(&desc, Some(&mut state))
            .map_err(|err| err.to_string())?;
    }

    state.ok_or_else(|| "blend state".to_string())
}

fn rasterizer(
    device: &ID3D11Device,
    cull: D3D11_CULL_MODE,
) -> Result<ID3D11RasterizerState, String> {
    let desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: cull,
        FrontCounterClockwise: true.into(),
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: true.into(),
        ScissorEnable: false.into(),
        MultisampleEnable: false.into(),
        AntialiasedLineEnable: false.into(),
    };
    let mut state = None;
    unsafe {
        device
            .CreateRasterizerState(&desc, Some(&mut state))
            .map_err(|err| err.to_string())?;
    }

    state.ok_or_else(|| "rasterizer".to_string())
}

struct DxUser {
    buffer: DynBuf,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    texture: Option<ID3D11ShaderResourceView>,
    sampler: Option<ID3D11SamplerState>,
    count: u32,
    depth: bool,
    constants: [f32; 16],
    rtv: Option<ID3D11RenderTargetView>,
    dsv: Option<ID3D11DepthStencilView>,
}

impl D3D11Window {
    fn flush_user(&mut self) {
        if self.user.is_empty() {
            return;
        }

        if self.user_cb.is_none() {
            self.user_cb = constant_buffer(&self.device, 64).ok();
        }

        let Some(constants) = self.user_cb.clone() else {
            return;
        };
        let mut idx = 0;

        while idx < self.user.len() {
            let vs = self.user[idx].vs.clone();
            let ps = self.user[idx].ps.clone();
            let layout = self.user[idx].layout.clone();
            let buffer = DynBuf {
                buffer: self.user[idx].buffer.buffer.clone(),
                capacity: self.user[idx].buffer.capacity,
                stride: self.user[idx].buffer.stride,
            };
            let count = self.user[idx].count;
            let depth = self.user[idx].depth;
            let texture = self.user[idx].texture.clone();
            let sampler = self.user[idx].sampler.clone();
            let words = self.user[idx].constants;
            let rtv = self.user[idx].rtv.clone();
            let dsv = self.user[idx].dsv.clone();
            let _ = write_constants(&self.context, &constants, &words);

            if let (Some(rtv), Some(dsv)) = (rtv, dsv) {
                unsafe {
                    self.context.OMSetRenderTargets(Some(&[Some(rtv)]), &dsv);
                }
            }
            self.draw_buffer(
                &vs,
                &ps,
                &layout,
                &constants,
                &buffer,
                count,
                if depth { &self.depth_on } else { &self.depth_off },
                &self.blend_on,
                &self.cull_none,
                texture,
                sampler,
            );
            idx += 1;
        }
    }
}

fn dx11_shader(device: &ID3D11Device, wgsl: &str) -> Result<crate::ui::gfx::Dx11Shader, String> {
    let cache = adapter_cache(device)?;
    let source = cache.hlsl(wgsl)?;
    let vs = shader::vs5(&source, s!("vs_main"))?;
    let ps = shader::ps5(&source, s!("fs_main"))?;

    Ok(crate::ui::gfx::Dx11Shader { vs, ps })
}

fn dx11_texture(
    device: &ID3D11Device,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<crate::ui::gfx::Dx11Texture, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width.max(1),
        Height: height.max(1),
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32 | D3D11_BIND_RENDER_TARGET.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let init = D3D11_SUBRESOURCE_DATA {
        pSysMem: pixels.as_ptr() as *const _,
        SysMemPitch: width.max(1) * 4,
        SysMemSlicePitch: 0,
    };
    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&desc, Some(&init), Some(&mut texture))
            .map_err(|err| err.to_string())?;
    }
    let texture = texture.ok_or_else(|| "texture".to_string())?;
    let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.ViewDimension = D3D_SRV_DIMENSION_TEXTURE2D;
    desc.Anonymous.Texture2D = D3D11_TEX2D_SRV {
        MostDetailedMip: 0,
        MipLevels: 1,
    };
    let mut view = None;
    unsafe {
        device
            .CreateShaderResourceView(&texture, Some(&desc), Some(&mut view))
            .map_err(|err| err.to_string())?;
    }

    Ok(crate::ui::gfx::Dx11Texture {
        texture,
        view: view.ok_or_else(|| "view".to_string())?,
    })
}

fn dx11_sampler(
    device: &ID3D11Device,
    linear: bool,
    repeat: bool,
) -> Result<ID3D11SamplerState, String> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: if linear {
            D3D11_FILTER_MIN_MAG_MIP_LINEAR
        } else {
            D3D11_FILTER_MIN_MAG_MIP_POINT
        },
        AddressU: if repeat {
            D3D11_TEXTURE_ADDRESS_WRAP
        } else {
            D3D11_TEXTURE_ADDRESS_CLAMP
        },
        AddressV: if repeat {
            D3D11_TEXTURE_ADDRESS_WRAP
        } else {
            D3D11_TEXTURE_ADDRESS_CLAMP
        },
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MipLODBias: 0.0,
        MaxAnisotropy: 1,
        ComparisonFunc: D3D11_COMPARISON_NEVER,
        BorderColor: [0.0; 4],
        MinLOD: 0.0,
        MaxLOD: f32::MAX,
    };
    let mut sampler = None;
    unsafe {
        device
            .CreateSamplerState(&desc, Some(&mut sampler))
            .map_err(|err| err.to_string())?;
    }

    sampler.ok_or_else(|| "sampler".to_string())
}

fn mesh_user_elements() -> [D3D11_INPUT_ELEMENT_DESC; 3] {
    [
        element(s!("LOC"), 0, DXGI_FORMAT_R32G32B32_FLOAT, 0),
        element(s!("LOC"), 1, DXGI_FORMAT_R32G32_FLOAT, 12),
        element(s!("LOC"), 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 20),
    ]
}

impl crate::ui::gfx::BackendGpu for D3D11Window {
    fn make_shader(&mut self, wgsl: &str) -> Result<crate::ui::gfx::Shader, String> {
        Ok(crate::ui::gfx::Shader::d3d11(dx11_shader(&self.device, wgsl)?))
    }

    fn make_texture(
        &mut self,
        image: &crate::world::surface::CpuImage,
    ) -> Result<crate::ui::gfx::Texture, String> {
        let pixels = crate::world::image_rgba(image);

        Ok(crate::ui::gfx::Texture::d3d11(dx11_texture(
            &self.device,
            &pixels,
            image.width,
            image.height,
        )?))
    }

    fn make_target(&mut self, width: u32, height: u32) -> Result<crate::ui::gfx::Target, String> {
        let pixels = vec![0u8; (width.max(1) as usize) * (height.max(1) as usize) * 4];
        let color = dx11_texture(&self.device, &pixels, width, height)?;
        let mut rtv = None;
        unsafe {
            self.device
                .CreateRenderTargetView(&color.texture, None, Some(&mut rtv))
                .map_err(|err| err.to_string())?;
        }
        let depth_desc = D3D11_TEXTURE2D_DESC {
            Width: width.max(1),
            Height: height.max(1),
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut depth = None;
        unsafe {
            self.device
                .CreateTexture2D(&depth_desc, None, Some(&mut depth))
                .map_err(|err| err.to_string())?;
        }
        let depth = depth.ok_or_else(|| "depth".to_string())?;
        let mut dsv = None;
        unsafe {
            self.device
                .CreateDepthStencilView(&depth, None, Some(&mut dsv))
                .map_err(|err| err.to_string())?;
        }

        Ok(crate::ui::gfx::Target::d3d11(crate::ui::gfx::Dx11Target {
            color: color.texture,
            view: color.view,
            rtv: rtv.ok_or_else(|| "rtv".to_string())?,
            depth,
            dsv: dsv.ok_or_else(|| "dsv".to_string())?,
            width,
            height,
        }))
    }

    fn make_buffer(&mut self, bytes: &[u8]) -> Result<crate::ui::gfx::Buffer, String> {
        let buffer = write_dynamic(&self.device, &self.context, None, bytes, 4)?;

        Ok(crate::ui::gfx::Buffer::d3d11(crate::ui::gfx::Dx11Buffer {
            buffer: buffer.buffer,
            bytes: bytes.len() as u32,
        }))
    }

    fn make_sampler(
        &mut self,
        linear: bool,
        repeat: bool,
    ) -> Result<crate::ui::gfx::Sampler, String> {
        Ok(crate::ui::gfx::Sampler::d3d11(crate::ui::gfx::Dx11Sampler {
            state: dx11_sampler(&self.device, linear, repeat)?,
        }))
    }

    fn make_pipeline(
        &mut self,
        shader: &crate::ui::gfx::Shader,
        screen: bool,
    ) -> Result<crate::ui::gfx::Pipeline, String> {
        let shader = shader.as_d3d11().ok_or_else(|| "shader".to_string())?;
        let vs = vertex_shader(&self.device, &shader.vs)?;
        let ps = pixel_shader(&self.device, &shader.ps)?;
        let elements = if screen {
            text_elements().to_vec()
        } else {
            mesh_user_elements().to_vec()
        };
        let layout = input_layout(&self.device, &shader.vs, &elements)?;
        let stride = if screen {
            crate::ui::gfx::SCREEN_FLOATS as u8
        } else {
            crate::ui::gfx::MESH_FLOATS as u8
        };

        Ok(crate::ui::gfx::Pipeline::d3d11(crate::ui::gfx::Dx11Pipeline {
            vs,
            ps,
            layout,
            stride,
            depth: !screen,
        }))
    }

    fn make_mesh(&mut self, verts: &[f32], screen: bool) -> Result<crate::ui::gfx::Mesh, String> {
        let bytes = bytes_of(verts);
        let stride = if screen { 32 } else { 36 };
        let buffer = write_dynamic(&self.device, &self.context, None, bytes, stride)?;

        Ok(crate::ui::gfx::Mesh::d3d11(crate::ui::gfx::Dx11Mesh {
            buffer: buffer.buffer,
            floats: verts.len() as u32,
            screen,
        }))
    }

    fn destroy_shader(&mut self, shader: crate::ui::gfx::Shader) {
        let _ = shader.into_d3d11();
    }

    fn destroy_texture(&mut self, texture: crate::ui::gfx::Texture) {
        let _ = texture.into_d3d11();
    }

    fn destroy_buffer(&mut self, buffer: crate::ui::gfx::Buffer) {
        let _ = buffer.into_d3d11();
    }

    fn destroy_sampler(&mut self, sampler: crate::ui::gfx::Sampler) {
        let _ = sampler.into_d3d11();
    }

    fn destroy_pipeline(&mut self, pipeline: crate::ui::gfx::Pipeline) {
        let _ = pipeline.into_d3d11();
    }

    fn destroy_target(&mut self, target: crate::ui::gfx::Target) {
        let _ = target.into_d3d11();
    }

    fn destroy_mesh(&mut self, mesh: crate::ui::gfx::Mesh) {
        let _ = mesh.into_d3d11();
    }

    fn draw_mesh(
        &mut self,
        mesh: &crate::ui::gfx::Mesh,
        pipeline: &crate::ui::gfx::Pipeline,
        texture: Option<&crate::ui::gfx::Texture>,
        sampler: Option<&crate::ui::gfx::Sampler>,
        view: &SceneView,
    ) {
        let (Some(mesh), Some(pipeline)) = (mesh.as_d3d11(), pipeline.as_d3d11()) else {
            return;
        };
        let stride = pipeline.stride.max(1) as u32;
        let count = mesh.floats / stride;
        let mut constants = [0.0; 16];
        constants.copy_from_slice(&view_proj(view));
        self.user.push(DxUser {
            buffer: DynBuf {
                buffer: mesh.buffer.clone(),
                capacity: mesh.floats * 4,
                stride: stride * 4,
            },
            vs: pipeline.vs.clone(),
            ps: pipeline.ps.clone(),
            layout: pipeline.layout.clone(),
            texture: texture.and_then(|item| item.as_d3d11()).map(|item| item.view.clone()),
            sampler: sampler.and_then(|item| item.as_d3d11()).map(|item| item.state.clone()),
            count,
            depth: pipeline.depth,
            constants,
            rtv: self.bound_rtv.clone(),
            dsv: self.bound_dsv.clone(),
        });
    }

    fn draw_sprite(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        texture: Option<&crate::ui::gfx::Texture>,
        pipeline: Option<&crate::ui::gfx::Pipeline>,
        sampler: Option<&crate::ui::gfx::Sampler>,
    ) {
        let verts = crate::ui::gfx::screen_quad(x, y, w, h, color);
        let bytes = bytes_of(&verts);
        let Ok(buffer) = write_dynamic(&self.device, &self.context, None, bytes, 32) else {
            return;
        };
        let (vs, ps, layout) = if let Some(pipeline) = pipeline.and_then(|item| item.as_d3d11()) {
            (pipeline.vs.clone(), pipeline.ps.clone(), pipeline.layout.clone())
        } else {
            (self.text_vs.clone(), self.text_ps.clone(), self.text_layout.clone())
        };
        let mut constants = [0.0; 16];
        constants[0] = self.width as f32;
        constants[1] = self.height as f32;
        self.user.push(DxUser {
            buffer,
            vs,
            ps,
            layout,
            texture: texture.and_then(|item| item.as_d3d11()).map(|item| item.view.clone()),
            sampler: sampler
                .and_then(|item| item.as_d3d11())
                .map(|item| item.state.clone())
                .or_else(|| Some(self.sampler.clone())),
            count: 6,
            depth: false,
            constants,
            rtv: self.bound_rtv.clone(),
            dsv: self.bound_dsv.clone(),
        });
    }

    fn builtin_shader(&mut self, index: u32) -> Option<crate::ui::gfx::Shader> {
        let source = match index {
            crate::ui::gfx::IDX_MESH => crate::ui::shaders::Program::Mesh.wgsl(),
            crate::ui::gfx::IDX_COLOR => crate::ui::shaders::Program::Color.wgsl(),
            crate::ui::gfx::IDX_TEXT => crate::ui::shaders::Program::Text.wgsl(),
            _ => return None,
        };

        dx11_shader(&self.device, &source)
            .ok()
            .map(crate::ui::gfx::Shader::d3d11)
    }

    fn builtin_pipeline(&mut self, index: u32) -> Option<crate::ui::gfx::Pipeline> {
        let (vs, ps, layout, stride, depth) = match index {
            crate::ui::gfx::IDX_MESH => (
                self.mesh_vs.clone(),
                self.mesh_ps.clone(),
                self.mesh_layout.clone(),
                0,
                true,
            ),
            crate::ui::gfx::IDX_COLOR => (
                self.color_vs.clone(),
                self.color_ps.clone(),
                self.color_layout.clone(),
                6,
                false,
            ),
            crate::ui::gfx::IDX_TEXT => (
                self.text_vs.clone(),
                self.text_ps.clone(),
                self.text_layout.clone(),
                crate::ui::gfx::SCREEN_FLOATS as u8,
                false,
            ),
            _ => return None,
        };

        Some(crate::ui::gfx::Pipeline::d3d11(crate::ui::gfx::Dx11Pipeline {
            vs,
            ps,
            layout,
            stride,
            depth,
        }))
    }

    fn builtin_texture(&mut self, _index: u32) -> Option<crate::ui::gfx::Texture> {
        None
    }

    fn builtin_sampler(&mut self, _index: u32) -> Option<crate::ui::gfx::Sampler> {
        Some(crate::ui::gfx::Sampler::d3d11(crate::ui::gfx::Dx11Sampler {
            state: self.sampler.clone(),
        }))
    }

    fn material_alias(&self, _name: &str) -> Option<crate::ui::gfx::Texture> {
        None
    }

    fn draw_buffer(
        &mut self,
        buffer: &crate::ui::gfx::Buffer,
        pipeline: &crate::ui::gfx::Pipeline,
        texture: Option<&crate::ui::gfx::Texture>,
        sampler: Option<&crate::ui::gfx::Sampler>,
        view: &SceneView,
    ) {
        let (Some(buffer), Some(pipeline)) = (buffer.as_d3d11(), pipeline.as_d3d11()) else {
            return;
        };
        let stride = pipeline.stride.max(1) as u32;
        let mut constants = [0.0; 16];
        constants.copy_from_slice(&view_proj(view));
        self.user.push(DxUser {
            buffer: DynBuf {
                buffer: buffer.buffer.clone(),
                capacity: buffer.bytes,
                stride: stride * 4,
            },
            vs: pipeline.vs.clone(),
            ps: pipeline.ps.clone(),
            layout: pipeline.layout.clone(),
            texture: texture.and_then(|item| item.as_d3d11()).map(|item| item.view.clone()),
            sampler: sampler.and_then(|item| item.as_d3d11()).map(|item| item.state.clone()),
            count: buffer.bytes / (stride * 4).max(1),
            depth: pipeline.depth,
            constants,
            rtv: self.bound_rtv.clone(),
            dsv: self.bound_dsv.clone(),
        });
    }

    fn draw_text_user(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        texture: Option<&crate::ui::gfx::Texture>,
        pipeline: Option<&crate::ui::gfx::Pipeline>,
        sampler: Option<&crate::ui::gfx::Sampler>,
    ) {
        if self.text_once.is_none() {
            self.text_once = crate::ui::batch::TextFrame::new().ok();
        }

        let Some(frame) = self.text_once.as_mut() else {
            return;
        };
        let verts = frame.capture(text, x, y, scale, color);
        let bytes = bytes_of(&verts);
        let Ok(buffer) = write_dynamic(&self.device, &self.context, None, bytes, 32) else {
            return;
        };
        let (vs, ps, layout) = if let Some(pipeline) = pipeline.and_then(|item| item.as_d3d11()) {
            (pipeline.vs.clone(), pipeline.ps.clone(), pipeline.layout.clone())
        } else {
            (self.text_vs.clone(), self.text_ps.clone(), self.text_layout.clone())
        };
        let mut constants = [0.0; 16];
        constants[0] = self.width as f32;
        constants[1] = self.height as f32;
        self.user.push(DxUser {
            buffer,
            vs,
            ps,
            layout,
            texture: texture.and_then(|item| item.as_d3d11()).map(|item| item.view.clone()),
            sampler: sampler
                .and_then(|item| item.as_d3d11())
                .map(|item| item.state.clone())
                .or_else(|| Some(self.sampler.clone())),
            count: (verts.len() / crate::ui::gfx::SCREEN_FLOATS) as u32,
            depth: false,
            constants,
            rtv: self.bound_rtv.clone(),
            dsv: self.bound_dsv.clone(),
        });
    }

    fn set_target(&mut self, target: Option<&crate::ui::gfx::Target>) {
        match target.and_then(|item| item.as_d3d11()) {
            Some(target) => {
                self.bound_rtv = Some(target.rtv.clone());
                self.bound_dsv = Some(target.dsv.clone());
            }
            None => {
                self.bound_rtv = None;
                self.bound_dsv = None;
            }
        }
    }

    fn target_bound(&self) -> bool {
        self.bound_rtv.is_some()
    }

    fn update_buffer(
        &mut self,
        buffer: crate::ui::gfx::Buffer,
        bytes: &[u8],
    ) -> crate::ui::gfx::Buffer {
        let Some(buffer) = buffer.into_d3d11() else {
            return self.make_buffer(bytes).unwrap_or_else(|_| {
                crate::ui::gfx::Buffer::d3d11(crate::ui::gfx::Dx11Buffer {
                    buffer: self.screen_cb.clone(),
                    bytes: 0,
                })
            });
        };
        let next = write_dynamic(
            &self.device,
            &self.context,
            Some(DynBuf {
                buffer: buffer.buffer,
                capacity: buffer.bytes,
                stride: 4,
            }),
            bytes,
            4,
        );

        match next {
            Ok(next) => crate::ui::gfx::Buffer::d3d11(crate::ui::gfx::Dx11Buffer {
                buffer: next.buffer,
                bytes: bytes.len() as u32,
            }),
            Err(_) => self.make_buffer(bytes).unwrap_or_else(|_| {
                crate::ui::gfx::Buffer::d3d11(crate::ui::gfx::Dx11Buffer {
                    buffer: self.screen_cb.clone(),
                    bytes: 0,
                })
            }),
        }
    }

    fn update_mesh(
        &mut self,
        mesh: crate::ui::gfx::Mesh,
        verts: &[f32],
    ) -> crate::ui::gfx::Mesh {
        let Some(mesh) = mesh.into_d3d11() else {
            return self.make_mesh(verts, false).unwrap_or_else(|_| {
                crate::ui::gfx::Mesh::d3d11(crate::ui::gfx::Dx11Mesh {
                    buffer: self.screen_cb.clone(),
                    floats: 0,
                    screen: false,
                })
            });
        };
        let screen = mesh.screen;
        let stride = if screen { 32 } else { 36 };
        let next = write_dynamic(
            &self.device,
            &self.context,
            Some(DynBuf {
                buffer: mesh.buffer,
                capacity: mesh.floats * 4,
                stride,
            }),
            bytes_of(verts),
            stride,
        );

        match next {
            Ok(next) => crate::ui::gfx::Mesh::d3d11(crate::ui::gfx::Dx11Mesh {
                buffer: next.buffer,
                floats: verts.len() as u32,
                screen,
            }),
            Err(_) => self.make_mesh(verts, screen).unwrap_or_else(|_| {
                crate::ui::gfx::Mesh::d3d11(crate::ui::gfx::Dx11Mesh {
                    buffer: self.screen_cb.clone(),
                    floats: 0,
                    screen,
                })
            }),
        }
    }

    fn update_texture(
        &mut self,
        texture: crate::ui::gfx::Texture,
        image: &crate::world::surface::CpuImage,
    ) -> crate::ui::gfx::Texture {
        let _ = texture.into_d3d11();

        self.make_texture(image).unwrap_or_else(|_| {
            crate::ui::gfx::Texture::d3d11(crate::ui::gfx::Dx11Texture {
                texture: self.depth.clone().unwrap_or_else(|| unsafe { self.swap.GetBuffer(0).unwrap() }),
                view: self.atlas_view.clone().unwrap_or_else(|| {
                    dx11_texture(&self.device, &[255, 255, 255, 255], 1, 1)
                        .unwrap()
                        .view
                }),
            })
        })
    }

    fn resize_target(
        &mut self,
        target: crate::ui::gfx::Target,
        width: u32,
        height: u32,
    ) -> Result<crate::ui::gfx::Target, String> {
        let _ = target.into_d3d11();

        self.make_target(width, height)
    }

    fn target_color(&self, target: &crate::ui::gfx::Target) -> Option<crate::ui::gfx::Texture> {
        target.as_d3d11().map(|target| {
            crate::ui::gfx::Texture::d3d11(crate::ui::gfx::Dx11Texture {
                texture: target.color.clone(),
                view: target.view.clone(),
            })
        })
    }
}
