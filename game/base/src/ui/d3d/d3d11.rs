use windows::core::{s, Interface};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use winit::event_loop::EventLoop;
use winit::window::Window as WinitWindow;

use crate::ui::d3d::draw::{
    bytes_of, grow, open_desktop, push_outline, push_rect, Desktop, TextFrame,
};
use crate::ui::d3d::math::view_proj;
use crate::ui::d3d::shader::{self, blob_bytes};
use crate::ui::voxel::SceneView;
use crate::ui::vr::{self, EyeViews, Headset, VrInput};
use crate::ui::window::Window;
use crate::ui::Color;

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
    pub fn try_new() -> Result<Self, String> {
        let desktop = open_desktop()?;
        let size = desktop.window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let (device, context, swap) = device_swap(desktop.hwnd, width, height)?;
        let (rtv, depth, dsv) = targets(&device, &swap, width, height)?;
        let mesh_vs_blob = shader::vs5(shader::MESH_SM5, s!("mesh_vert"))?;
        let mesh_ps_blob = shader::ps5(shader::MESH_SM5, s!("mesh_frag"))?;
        let color_vs_blob = shader::vs5(shader::COLOR_SM5, s!("color_vert"))?;
        let color_ps_blob = shader::ps5(shader::COLOR_SM5, s!("color_frag"))?;
        let text_vs_blob = shader::vs5(shader::TEXT_SM5, s!("text_vert"))?;
        let text_ps_blob = shader::ps5(shader::TEXT_SM5, s!("text_frag"))?;
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
            24,
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
    fn create_window() -> Self {
        Self::try_new().expect("d3d11")
    }

    fn set_window_title(&mut self, title: &str) {
        self.desktop.window.set_title(title);
    }

    fn set_size(&mut self, w: u32, h: u32) {
        let _ = self
            .desktop
            .window
            .request_inner_size(winit::dpi::PhysicalSize::new(w, h));

        if let Err(err) = self.resize(w, h) {
            println!("[gfx] d3d11 resize {err}");
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

        if let Some(eyes) = self.eye_views {
            self.view = view_proj(&eyes.views[0]);
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
            );
        }

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
                self.context
                    .PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            }

            self.context.Draw(vertices, 0);
        }
    }
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
    format: DXGI_FORMAT,
    offset: u32,
) -> D3D11_INPUT_ELEMENT_DESC {
    D3D11_INPUT_ELEMENT_DESC {
        SemanticName: name,
        SemanticIndex: 0,
        Format: format,
        InputSlot: 0,
        AlignedByteOffset: offset,
        InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
        InstanceDataStepRate: 0,
    }
}

fn mesh_elements() -> [D3D11_INPUT_ELEMENT_DESC; 2] {
    [
        element(s!("POSITION"), DXGI_FORMAT_R32G32B32_FLOAT, 0),
        element(s!("COLOR"), DXGI_FORMAT_R32G32B32_FLOAT, 12),
    ]
}

fn color_elements() -> [D3D11_INPUT_ELEMENT_DESC; 2] {
    [
        element(s!("POSITION"), DXGI_FORMAT_R32G32_FLOAT, 0),
        element(s!("COLOR"), DXGI_FORMAT_R32G32B32A32_FLOAT, 8),
    ]
}

fn text_elements() -> [D3D11_INPUT_ELEMENT_DESC; 3] {
    [
        element(s!("POSITION"), DXGI_FORMAT_R32G32_FLOAT, 0),
        element(s!("TEXCOORD"), DXGI_FORMAT_R32G32_FLOAT, 8),
        element(s!("COLOR"), DXGI_FORMAT_R32G32B32A32_FLOAT, 16),
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
