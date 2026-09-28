use std::mem::ManuallyDrop;

use windows::core::{s, Interface, PCSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, RECT};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Threading::{CreateEventA, WaitForSingleObject, INFINITE};
use winit::event_loop::EventLoop;
use winit::window::Window as WinitWindow;

use crate::ui::d3d::draw::{bytes_of, open_desktop, push_outline, push_rect, Desktop, TextFrame};
use crate::ui::d3d::math::view_proj;
use crate::ui::d3d::shader::{self, blob_bytes, blob_text};
use crate::ui::vr::{self, EyeViews, Headset, VrInput};
use crate::ui::voxel::SceneView;
use crate::ui::window::Window;
use crate::ui::Color;

const FRAMES: u32 = 2;

pub struct D3D12Window {
    desktop: Desktop,
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    swap: IDXGISwapChain3,
    swap_flags: DXGI_SWAP_CHAIN_FLAG,
    present_interval: u32,
    present_flags: DXGI_PRESENT,
    targets: Vec<ID3D12Resource>,
    rtv_heap: ID3D12DescriptorHeap,
    rtv_stride: usize,
    depth: Option<ID3D12Resource>,
    dsv_heap: ID3D12DescriptorHeap,
    dsv: D3D12_CPU_DESCRIPTOR_HANDLE,
    srv_heap: ID3D12DescriptorHeap,
    srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
    allocator: ID3D12CommandAllocator,
    commands: ID3D12GraphicsCommandList,
    plain_root: ID3D12RootSignature,
    text_root: ID3D12RootSignature,
    mesh_pso: ID3D12PipelineState,
    color_pso: ID3D12PipelineState,
    text_pso: ID3D12PipelineState,
    mesh: Option<ID3D12Resource>,
    mesh_capacity: u64,
    ui_buf: Option<ID3D12Resource>,
    ui_capacity: u64,
    text_buf: Option<ID3D12Resource>,
    text_capacity: u64,
    atlas: Option<ID3D12Resource>,
    atlas_upload: Option<ID3D12Resource>,
    atlas_pitch: u32,
    atlas_size: (u32, u32),
    atlas_shader: bool,
    copy_atlas: bool,
    fence: ID3D12Fence,
    fence_event: HANDLE,
    submitted: u64,
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
    eyes: Option<Eyes12>,
    eye_views: Option<EyeViews>,
}

struct Eyes12 {
    width: u32,
    height: u32,
    color: [ID3D12Resource; 2],
    depth: [ID3D12Resource; 2],
    rtv_heap: ID3D12DescriptorHeap,
    dsv_heap: ID3D12DescriptorHeap,
    rtv_stride: usize,
    dsv_stride: usize,
    as_shader: bool,
}

impl D3D12Window {
    pub fn try_new() -> Result<Self, String> {
        enable_debug();
        let desktop = open_desktop()?;
        let size = desktop.window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let factory = factory()?;
        let adapter = adapter(&factory)?;
        let mut device_slot: Option<ID3D12Device> = None;
        unsafe {
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device_slot).map_err(|err| err.to_string())?;
        }
        let device = device_slot.ok_or_else(|| "d3d12 device".to_string())?;
        let queue: ID3D12CommandQueue = unsafe {
            device
                .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                    Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                    Priority: 0,
                    Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
                    NodeMask: 0,
                })
                .map_err(|err| err.to_string())?
        };
        let (swap, swap_flags, present_interval, present_flags) = swap_chain(&factory, &queue, desktop.hwnd, width, height)?;
        let _ = unsafe { factory.MakeWindowAssociation(desktop.hwnd, DXGI_MWA_NO_ALT_ENTER) };
        let rtv_heap = descriptor_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_RTV, FRAMES, D3D12_DESCRIPTOR_HEAP_FLAGS(0))?;
        let rtv_stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) } as usize;
        let dsv_heap = descriptor_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_DSV, 1, D3D12_DESCRIPTOR_HEAP_FLAGS(0))?;
        let srv_heap = descriptor_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 1, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE)?;
        let srv_gpu = unsafe { srv_heap.GetGPUDescriptorHandleForHeapStart() };
        let plain_root = plain_root(&device)?;
        let text_root = text_root(&device)?;
        let mesh_vs = shader::vs5(shader::MESH_SM5, s!("mesh_vert"))?;
        let mesh_ps = shader::ps5(shader::MESH_SM5, s!("mesh_frag"))?;
        let color_vs = shader::vs5(shader::COLOR_SM5, s!("color_vert"))?;
        let color_ps = shader::ps5(shader::COLOR_SM5, s!("color_frag"))?;
        let text_vs = shader::vs5(shader::TEXT_SM5, s!("text_vert"))?;
        let text_ps = shader::ps5(shader::TEXT_SM5, s!("text_frag"))?;
        let mesh_pso = pipeline(&device, &plain_root, &mesh_vs, &mesh_ps, &mesh_elements(), false, true, D3D12_CULL_MODE_BACK)?;
        let color_pso = pipeline(&device, &plain_root, &color_vs, &color_ps, &color_elements(), true, false, D3D12_CULL_MODE_NONE)?;
        let text_pso = pipeline(&device, &text_root, &text_vs, &text_ps, &text_elements(), true, false, D3D12_CULL_MODE_NONE)?;
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).map_err(|err| err.to_string())? };
        let commands: ID3D12GraphicsCommandList = unsafe {
            device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, &mesh_pso)
                .map_err(|err| err.to_string())?
        };
        unsafe { commands.Close().map_err(|err| err.to_string())?; }
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(|err| err.to_string())? };
        let fence_event = unsafe { CreateEventA(None, false, false, PCSTR::null()).map_err(|err| err.to_string())? };
        let text = TextFrame::new()?;
        let mut window = Self {
            desktop,
            device,
            queue,
            swap,
            swap_flags,
            present_interval,
            present_flags,
            targets: Vec::new(),
            rtv_heap,
            rtv_stride,
            depth: None,
            dsv_heap,
            dsv: D3D12_CPU_DESCRIPTOR_HANDLE { ptr: 0 },
            srv_heap,
            srv_gpu,
            allocator,
            commands,
            plain_root,
            text_root,
            mesh_pso,
            color_pso,
            text_pso,
            mesh: None,
            mesh_capacity: 0,
            ui_buf: None,
            ui_capacity: 0,
            text_buf: None,
            text_capacity: 0,
            atlas: None,
            atlas_upload: None,
            atlas_pitch: 0,
            atlas_size: (0, 0),
            atlas_shader: false,
            copy_atlas: false,
            fence,
            fence_event,
            submitted: 0,
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
        };
        window.create_targets()?;
        window.create_depth(width, height)?;

        Ok(window)
    }

    fn wait(&self) {
        if self.submitted == 0 {
            return;
        }

        unsafe {
            if self.fence.GetCompletedValue() < self.submitted {
                let _ = self.fence.SetEventOnCompletion(self.submitted, self.fence_event);
                WaitForSingleObject(self.fence_event, INFINITE);
            }
        }
    }

    fn signal(&mut self) {
        self.submitted += 1;
        let _ = unsafe { self.queue.Signal(&self.fence, self.submitted) };
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        let width = width.max(1);
        let height = height.max(1);

        if self.width == width && self.height == height && !self.targets.is_empty() {
            return Ok(());
        }

        self.wait();
        self.targets.clear();
        self.depth = None;
        unsafe {
            self.swap
                .ResizeBuffers(FRAMES, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, self.swap_flags)
                .map_err(|err| err.to_string())?;
        }
        self.width = width;
        self.height = height;
        self.create_targets()?;
        self.create_depth(width, height)?;

        Ok(())
    }

    fn create_targets(&mut self) -> Result<(), String> {
        self.targets.clear();
        let start = unsafe { self.rtv_heap.GetCPUDescriptorHandleForHeapStart() };
        let mut idx = 0;

        while idx < FRAMES {
            let texture: ID3D12Resource = unsafe { self.swap.GetBuffer(idx).map_err(|err| err.to_string())? };
            let handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + idx as usize * self.rtv_stride };
            unsafe { self.device.CreateRenderTargetView(&texture, None, handle); }
            self.targets.push(texture);
            idx += 1;
        }

        Ok(())
    }

    fn create_depth(&mut self, width: u32, height: u32) -> Result<(), String> {
        let desc = texture_desc(width, height, DXGI_FORMAT_D32_FLOAT, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL);
        let clear = depth_clear();
        self.depth = Some(committed(&self.device, &heap(D3D12_HEAP_TYPE_DEFAULT), &desc, D3D12_RESOURCE_STATE_DEPTH_WRITE, Some(&clear))?);
        self.dsv = unsafe { self.dsv_heap.GetCPUDescriptorHandleForHeapStart() };
        unsafe { self.device.CreateDepthStencilView(self.depth.as_ref().unwrap(), None, self.dsv); }

        Ok(())
    }

    fn upload_mesh(&mut self, vertices: &[f32]) -> Result<(), String> {
        ensure_upload(&self.device, &mut self.mesh, &mut self.mesh_capacity, bytes_of(vertices))
    }

    fn prepare(&mut self) -> Result<(), String> {
        ensure_upload(&self.device, &mut self.ui_buf, &mut self.ui_capacity, bytes_of(&self.ui))?;
        ensure_upload(&self.device, &mut self.text_buf, &mut self.text_capacity, bytes_of(&self.text.verts))?;

        if self.text.dirty || self.atlas_size != self.text.size || self.atlas.is_none() {
            if self.atlas_size != self.text.size || self.atlas.is_none() {
                let desc = texture_desc(self.text.size.0, self.text.size.1, DXGI_FORMAT_R8_UNORM, D3D12_RESOURCE_FLAGS(0));
                self.atlas = Some(committed(&self.device, &heap(D3D12_HEAP_TYPE_DEFAULT), &desc, D3D12_RESOURCE_STATE_COPY_DEST, None)?);
                self.atlas_shader = false;
                self.atlas_size = self.text.size;
                self.atlas_pitch = align(self.text.size.0, D3D12_TEXTURE_DATA_PITCH_ALIGNMENT);
                let upload_bytes = self.atlas_pitch as u64 * self.text.size.1 as u64;
                self.atlas_upload = Some(upload_buffer(&self.device, upload_bytes.max(1))?);
                write_srv(&self.device, &self.srv_heap, self.atlas.as_ref().unwrap());
            }

            fill_atlas(self.atlas_upload.as_ref().unwrap(), &self.text.pixels, self.text.size, self.atlas_pitch)?;
            self.copy_atlas = true;
            self.text.dirty = false;
        }

        Ok(())
    }

    fn record(&mut self) -> Result<(), String> {
        self.prepare()?;
        let copied = self.copy_atlas;
        unsafe {
            self.allocator.Reset().map_err(|err| err.to_string())?;
            self.commands.Reset(&self.allocator, &self.mesh_pso).map_err(|err| err.to_string())?;
        }
        self.encode_eyes()?;
        let encoded = self.encode();
        unsafe { self.commands.Close().map_err(|err| err.to_string())?; }
        encoded?;

        if copied {
            self.copy_atlas = false;
            self.atlas_shader = true;
        }

        let list: ID3D12CommandList = self.commands.cast().map_err(|err| err.to_string())?;
        unsafe { self.queue.ExecuteCommandLists(&[Some(list)]); }
        self.signal();

        if self.eye_views.is_some() {
            if let Some(eyes) = self.eyes.as_mut() {
                eyes.as_shader = true;
            }
        }

        self.submit_eyes();
        let _ = unsafe { self.swap.Present(self.present_interval, self.present_flags) };

        if let Some(headset) = self.vr.as_mut() {
            headset.handoff();
        }

        Ok(())
    }

    fn ensure_eyes(&mut self, width: u32, height: u32) -> Result<(), String> {
        let width = width.max(1);
        let height = height.max(1);

        if let Some(eyes) = &self.eyes {
            if eyes.width == width && eyes.height == height {
                return Ok(());
            }
        }

        let rtv_heap = descriptor_heap(&self.device, D3D12_DESCRIPTOR_HEAP_TYPE_RTV, 2, D3D12_DESCRIPTOR_HEAP_FLAGS(0))?;
        let dsv_heap = descriptor_heap(&self.device, D3D12_DESCRIPTOR_HEAP_TYPE_DSV, 2, D3D12_DESCRIPTOR_HEAP_FLAGS(0))?;
        let rtv_stride = unsafe { self.device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) } as usize;
        let dsv_stride = unsafe { self.device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV) } as usize;
        let color_desc = texture_desc(width, height, DXGI_FORMAT_R8G8B8A8_UNORM, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET);
        let depth_desc = texture_desc(width, height, DXGI_FORMAT_D32_FLOAT, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL);
        let clear = depth_clear();
        let color0 = committed(&self.device, &heap(D3D12_HEAP_TYPE_DEFAULT), &color_desc, D3D12_RESOURCE_STATE_RENDER_TARGET, None)?;
        let color1 = committed(&self.device, &heap(D3D12_HEAP_TYPE_DEFAULT), &color_desc, D3D12_RESOURCE_STATE_RENDER_TARGET, None)?;
        let depth0 = committed(&self.device, &heap(D3D12_HEAP_TYPE_DEFAULT), &depth_desc, D3D12_RESOURCE_STATE_DEPTH_WRITE, Some(&clear))?;
        let depth1 = committed(&self.device, &heap(D3D12_HEAP_TYPE_DEFAULT), &depth_desc, D3D12_RESOURCE_STATE_DEPTH_WRITE, Some(&clear))?;
        let rtv_start = unsafe { rtv_heap.GetCPUDescriptorHandleForHeapStart() };
        let dsv_start = unsafe { dsv_heap.GetCPUDescriptorHandleForHeapStart() };
        unsafe {
            self.device.CreateRenderTargetView(&color0, None, rtv_start);
            self.device.CreateRenderTargetView(&color1, None, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv_start.ptr + rtv_stride });
            self.device.CreateDepthStencilView(&depth0, None, dsv_start);
            self.device.CreateDepthStencilView(&depth1, None, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: dsv_start.ptr + dsv_stride });
        }
        self.eyes = Some(Eyes12 {
            width,
            height,
            color: [color0, color1],
            depth: [depth0, depth1],
            rtv_heap,
            dsv_heap,
            rtv_stride,
            dsv_stride,
            as_shader: false,
        });

        Ok(())
    }

    fn encode_eyes(&mut self) -> Result<(), String> {
        let Some(frame) = self.eye_views else {
            return Ok(());
        };
        let Some(eyes) = self.eyes.as_ref() else {
            return Ok(());
        };
        let colors = [eyes.color[0].clone(), eyes.color[1].clone()];
        let from_shader = eyes.as_shader;
        let mesh = self.mesh.clone();
        let vertices = self.mesh_vertices;
        let draw_mesh = self.draw_mesh;
        let pso = self.mesh_pso.clone();
        let root = self.plain_root.clone();
        let clear = self.clear;
        unsafe { self.commands.SetDescriptorHeaps(&[Some(self.srv_heap.clone())]); }
        let mut idx = 0;

        while idx < 2 {
            let rtv = self.eye_rtv(idx);
            let dsv = self.eye_dsv(idx);
            unsafe {
                if from_shader {
                    self.commands.ResourceBarrier(&[transition(&colors[idx], D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET)]);
                }

                self.commands.RSSetViewports(&[D3D12_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: frame.width as f32,
                    Height: frame.height as f32,
                    MinDepth: D3D12_MIN_DEPTH,
                    MaxDepth: D3D12_MAX_DEPTH,
                }]);
                self.commands.RSSetScissorRects(&[RECT {
                    left: 0,
                    top: 0,
                    right: frame.width as i32,
                    bottom: frame.height as i32,
                }]);
                self.commands.OMSetRenderTargets(1, Some(&rtv), false, Some(&dsv));
                self.commands.ClearRenderTargetView(rtv, &clear, None);
                self.commands.ClearDepthStencilView(dsv, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);
                self.commands.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            }

            if draw_mesh {
                if let Some(buffer) = mesh.as_ref() {
                    let matrix = view_proj(&frame.views[idx]);
                    self.draw(buffer, 24, vertices, &pso, &root, &matrix, false);
                }
            }

            unsafe {
                self.commands.ResourceBarrier(&[transition(&colors[idx], D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE)]);
            }
            idx += 1;
        }

        Ok(())
    }

    fn submit_eyes(&mut self) {
        if self.eye_views.is_none() {
            return;
        }

        let (color, queue) = {
            let Some(eyes) = self.eyes.as_ref() else {
                return;
            };

            ([eyes.color[0].clone(), eyes.color[1].clone()], self.queue.clone())
        };
        let Some(headset) = self.vr.as_mut() else {
            return;
        };
        headset.submit_d3d12(0, color[0].as_raw(), queue.as_raw());
        headset.submit_d3d12(1, color[1].as_raw(), queue.as_raw());
    }

    fn eye_rtv(&self, index: usize) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        let eyes = self.eyes.as_ref().unwrap();
        let start = unsafe { eyes.rtv_heap.GetCPUDescriptorHandleForHeapStart() };

        D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + index * eyes.rtv_stride }
    }

    fn eye_dsv(&self, index: usize) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        let eyes = self.eyes.as_ref().unwrap();
        let start = unsafe { eyes.dsv_heap.GetCPUDescriptorHandleForHeapStart() };

        D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + index * eyes.dsv_stride }
    }

    fn encode(&self) -> Result<(), String> {
        let frame = unsafe { self.swap.GetCurrentBackBufferIndex() } as usize;
        let target = self.targets.get(frame).ok_or_else(|| "back buffer".to_string())?;
        let list = &self.commands;
        unsafe {
            list.SetDescriptorHeaps(&[Some(self.srv_heap.clone())]);
            list.RSSetViewports(&[D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: D3D12_MIN_DEPTH,
                MaxDepth: D3D12_MAX_DEPTH,
            }]);
            list.RSSetScissorRects(&[RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            }]);
            list.ResourceBarrier(&[transition(target, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET)]);
        }

        if self.copy_atlas {
            let atlas = self.atlas.as_ref().ok_or_else(|| "atlas".to_string())?;
            let upload = self.atlas_upload.as_ref().ok_or_else(|| "atlas upload".to_string())?;

            if self.atlas_shader {
                unsafe { list.ResourceBarrier(&[transition(atlas, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST)]); }
            }

            let src = footprint_location(upload, self.atlas_size, self.atlas_pitch);
            let dst = index_location(atlas);
            unsafe { list.CopyTextureRegion(&dst, 0, 0, 0, &src, None); }
            unsafe { list.ResourceBarrier(&[transition(atlas, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE)]); }
        }

        let rtv = self.rtv(frame as u32);
        unsafe {
            list.OMSetRenderTargets(1, Some(&rtv), false, Some(&self.dsv));
            list.ClearRenderTargetView(rtv, &self.clear, None);
            list.ClearDepthStencilView(self.dsv, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        }

        if self.draw_mesh {
            if let Some(mesh) = self.mesh.as_ref() {
                self.draw(mesh, 24, self.mesh_vertices, &self.mesh_pso, &self.plain_root, &self.view, false);
            }
        }

        if let Some(ui) = self.ui_buf.as_ref() {
            let mut screen = [0.0; 16];
            screen[0] = self.width as f32;
            screen[1] = self.height as f32;
            self.draw(ui, 24, (self.ui.len() / 6) as u32, &self.color_pso, &self.plain_root, &screen, false);
        }

        if let Some(text) = self.text_buf.as_ref() {
            let mut screen = [0.0; 16];
            screen[0] = self.width as f32;
            screen[1] = self.height as f32;
            self.draw(text, 32, (self.text.verts.len() / 8) as u32, &self.text_pso, &self.text_root, &screen, true);
        }

        unsafe { list.ResourceBarrier(&[transition(target, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT)]); }

        Ok(())
    }

    fn draw(&self, buffer: &ID3D12Resource, stride: u32, vertices: u32, pso: &ID3D12PipelineState, root: &ID3D12RootSignature, constants: &[f32; 16], textured: bool) {
        if vertices == 0 {
            return;
        }

        let view = D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: unsafe { buffer.GetGPUVirtualAddress() },
            SizeInBytes: vertices * stride,
            StrideInBytes: stride,
        };
        let list = &self.commands;
        unsafe {
            list.SetGraphicsRootSignature(root);
            list.SetPipelineState(pso);
            list.SetGraphicsRoot32BitConstants(0, 16, constants.as_ptr() as *const _, 0);

            if textured {
                list.SetGraphicsRootDescriptorTable(1, self.srv_gpu);
            }

            list.IASetVertexBuffers(0, Some(&[view]));
            list.DrawInstanced(vertices, 1, 0, 0);
        }
    }

    fn rtv(&self, index: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        let start = unsafe { self.rtv_heap.GetCPUDescriptorHandleForHeapStart() };

        D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + index as usize * self.rtv_stride }
    }
}

impl Window for D3D12Window {
    fn create_window() -> Self {
        Self::try_new().expect("d3d12")
    }

    fn set_window_title(&mut self, title: &str) {
        self.desktop.window.set_title(title);
    }

    fn set_size(&mut self, w: u32, h: u32) {
        let _ = self.desktop.window.request_inner_size(winit::dpi::PhysicalSize::new(w, h));

        if let Err(err) = self.resize(w, h) {
            println!("[gfx] d3d12 resize {err}");
        }
    }

    fn winit_window(&self) -> &WinitWindow {
        &self.desktop.window
    }

    fn take_event_loop(&mut self) -> EventLoop<()> {
        self.desktop.event_loop.take().expect("Event loop missing")
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        self.wait();
        self.clear = [red, green, blue, 1.0];
        self.ui.clear();
        self.draw_mesh = false;
    }

    fn draw_colored_mesh(&mut self, vertices: &[f32], revision: u64, view: &SceneView) {
        self.view = view_proj(view);
        self.eye_views = vr::connect(&mut self.vr, &mut self.vr_failed, self.vr_enable, view);

        if let Some(frame) = self.eye_views {
            self.view = view_proj(&frame.views[0]);

            if let Err(err) = self.ensure_eyes(frame.width, frame.height) {
                println!("[vr] eyes {err}");
                self.eye_views = None;
                self.view = view_proj(view);
            }
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
        if self.width == 0 || self.height == 0 {
            return;
        }

        if let Err(err) = self.record() {
            println!("[gfx] d3d12 {err}");
        }
    }
}

impl Drop for D3D12Window {
    fn drop(&mut self) {
        self.wait();
        self.vr.take();
        unsafe {
            let _ = CloseHandle(self.fence_event);
        }
    }
}

fn enable_debug() {
    if !cfg!(debug_assertions) {
        return;
    }

    unsafe {
        let mut debug: Option<ID3D12Debug> = None;

        if D3D12GetDebugInterface(&mut debug).is_ok() {
            if let Some(debug) = debug {
                debug.EnableDebugLayer();
            }
        }
    }
}

fn factory() -> Result<IDXGIFactory4, String> {
    unsafe {
        if cfg!(debug_assertions) {
            if let Ok(factory) = CreateDXGIFactory2(DXGI_CREATE_FACTORY_DEBUG) {
                return Ok(factory);
            }
        }

        CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).map_err(|err| err.to_string())
    }
}

fn adapter(factory: &IDXGIFactory4) -> Result<IDXGIAdapter1, String> {
    let mut idx = 0;

    loop {
        let adapter = match unsafe { factory.EnumAdapters1(idx) } {
            Ok(adapter) => adapter,
            Err(_) => break,
        };
        idx += 1;
        let desc = unsafe { adapter.GetDesc1().map_err(|err| err.to_string())? };

        if (desc.Flags as i32) & DXGI_ADAPTER_FLAG_SOFTWARE.0 != 0 {
            continue;
        }

        if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, std::ptr::null_mut::<Option<ID3D12Device>>()) }.is_ok() {
            return Ok(adapter);
        }
    }

    unsafe { factory.EnumWarpAdapter().map_err(|err| err.to_string()) }
}

fn swap_chain(
    factory: &IDXGIFactory4,
    queue: &ID3D12CommandQueue,
    hwnd: windows::Win32::Foundation::HWND,
    width: u32,
    height: u32,
) -> Result<(IDXGISwapChain3, DXGI_SWAP_CHAIN_FLAG, u32, DXGI_PRESENT), String> {
    let mut desc = swap_desc(width, height, DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32);
    let created = unsafe { factory.CreateSwapChainForHwnd(queue, hwnd, &desc, None, None) };

    if let Ok(chain) = created {
        let swap: IDXGISwapChain3 = chain.cast().map_err(|err| err.to_string())?;

        return Ok((swap, DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING, 0, DXGI_PRESENT_ALLOW_TEARING));
    }

    desc.Flags = 0;
    let chain = unsafe { factory.CreateSwapChainForHwnd(queue, hwnd, &desc, None, None).map_err(|err| err.to_string())? };
    let swap: IDXGISwapChain3 = chain.cast().map_err(|err| err.to_string())?;

    Ok((swap, DXGI_SWAP_CHAIN_FLAG(0), 1, DXGI_PRESENT(0)))
}

fn swap_desc(width: u32, height: u32, flags: u32) -> DXGI_SWAP_CHAIN_DESC1 {
    DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: FRAMES,
        Scaling: DXGI_SCALING_NONE,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: flags,
    }
}

fn descriptor_heap(device: &ID3D12Device, kind: D3D12_DESCRIPTOR_HEAP_TYPE, count: u32, flags: D3D12_DESCRIPTOR_HEAP_FLAGS) -> Result<ID3D12DescriptorHeap, String> {
    unsafe {
        device
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: kind,
                NumDescriptors: count,
                Flags: flags,
                NodeMask: 0,
            })
            .map_err(|err| err.to_string())
    }
}

fn heap(kind: D3D12_HEAP_TYPE) -> D3D12_HEAP_PROPERTIES {
    D3D12_HEAP_PROPERTIES {
        Type: kind,
        CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
        MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
        CreationNodeMask: 0,
        VisibleNodeMask: 0,
    }
}

fn texture_desc(width: u32, height: u32, format: DXGI_FORMAT, flags: D3D12_RESOURCE_FLAGS) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Alignment: 0,
        Width: width.max(1) as u64,
        Height: height.max(1),
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: flags,
    }
}

fn buffer_desc(bytes: u64) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Alignment: 0,
        Width: bytes.max(1),
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: D3D12_RESOURCE_FLAGS(0),
    }
}

fn committed(
    device: &ID3D12Device,
    heap: &D3D12_HEAP_PROPERTIES,
    desc: &D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
    clear: Option<&D3D12_CLEAR_VALUE>,
) -> Result<ID3D12Resource, String> {
    let mut resource = None;
    unsafe {
        device
            .CreateCommittedResource(heap, D3D12_HEAP_FLAG_NONE, desc, state, clear.map(|value| value as *const _), &mut resource)
            .map_err(|err| err.to_string())?;
    }

    resource.ok_or_else(|| "resource".to_string())
}

fn upload_buffer(device: &ID3D12Device, bytes: u64) -> Result<ID3D12Resource, String> {
    committed(device, &heap(D3D12_HEAP_TYPE_UPLOAD), &buffer_desc(bytes), D3D12_RESOURCE_STATE_GENERIC_READ, None)
}

fn ensure_upload(device: &ID3D12Device, slot: &mut Option<ID3D12Resource>, capacity: &mut u64, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Ok(());
    }

    if slot.is_none() || *capacity < bytes.len() as u64 {
        let size = grow_u64(*capacity, bytes.len() as u64);
        *slot = Some(upload_buffer(device, size)?);
        *capacity = size;
    }

    write_mapped(slot.as_ref().unwrap(), bytes)
}

fn write_mapped(resource: &ID3D12Resource, bytes: &[u8]) -> Result<(), String> {
    unsafe {
        let mut data = std::ptr::null_mut();
        resource.Map(0, None, Some(&mut data)).map_err(|err| err.to_string())?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data as *mut u8, bytes.len());
        resource.Unmap(0, None);
    }

    Ok(())
}

fn fill_atlas(resource: &ID3D12Resource, pixels: &[u8], size: (u32, u32), pitch: u32) -> Result<(), String> {
    unsafe {
        let mut data = std::ptr::null_mut();
        resource.Map(0, None, Some(&mut data)).map_err(|err| err.to_string())?;
        let dst = data as *mut u8;
        let mut row = 0;

        while row < size.1 {
            let src = (row * size.0) as usize;
            let out = dst.add((row * pitch) as usize);
            std::ptr::copy_nonoverlapping(pixels.as_ptr().add(src), out, size.0 as usize);
            row += 1;
        }

        resource.Unmap(0, None);
    }

    Ok(())
}

fn write_srv(device: &ID3D12Device, heap: &ID3D12DescriptorHeap, texture: &ID3D12Resource) {
    let mut desc = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
    desc.Format = DXGI_FORMAT_R8_UNORM;
    desc.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    desc.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    desc.Anonymous.Texture2D = D3D12_TEX2D_SRV {
        MostDetailedMip: 0,
        MipLevels: 1,
        PlaneSlice: 0,
        ResourceMinLODClamp: 0.0,
    };
    let cpu = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
    unsafe { device.CreateShaderResourceView(texture, Some(&desc), cpu); }
}

fn depth_clear() -> D3D12_CLEAR_VALUE {
    let mut clear = D3D12_CLEAR_VALUE {
        Format: DXGI_FORMAT_D32_FLOAT,
        Anonymous: D3D12_CLEAR_VALUE_0::default(),
    };
    clear.Anonymous.DepthStencil = D3D12_DEPTH_STENCIL_VALUE { Depth: 1.0, Stencil: 0 };

    clear
}

fn transition(resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(unsafe { std::mem::transmute_copy(resource) })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

fn footprint_location(resource: &ID3D12Resource, size: (u32, u32), pitch: u32) -> D3D12_TEXTURE_COPY_LOCATION {
    let mut location = D3D12_TEXTURE_COPY_LOCATION::default();
    location.pResource = ManuallyDrop::new(Some(unsafe { std::mem::transmute_copy(resource) }));
    location.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    location.Anonymous.PlacedFootprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
        Offset: 0,
        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
            Format: DXGI_FORMAT_R8_UNORM,
            Width: size.0,
            Height: size.1,
            Depth: 1,
            RowPitch: pitch,
        },
    };

    location
}

fn index_location(resource: &ID3D12Resource) -> D3D12_TEXTURE_COPY_LOCATION {
    let mut location = D3D12_TEXTURE_COPY_LOCATION::default();
    location.pResource = ManuallyDrop::new(Some(unsafe { std::mem::transmute_copy(resource) }));
    location.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    location.Anonymous.SubresourceIndex = 0;

    location
}

fn align(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) & !(alignment - 1)
}

fn grow_u64(current: u64, needed: u64) -> u64 {
    let mut size = current.max(256);

    while size < needed {
        size = size.saturating_mul(2);
    }

    size
}

fn plain_root(device: &ID3D12Device) -> Result<ID3D12RootSignature, String> {
    let params = [constants_param(16)];

    signature(device, &params, &[])
}

fn text_root(device: &ID3D12Device) -> Result<ID3D12RootSignature, String> {
    let range = D3D12_DESCRIPTOR_RANGE {
        RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
        NumDescriptors: 1,
        BaseShaderRegister: 0,
        RegisterSpace: 0,
        OffsetInDescriptorsFromTableStart: 0,
    };
    let params = [constants_param(16), table_param(&range)];
    let samplers = [static_sampler()];

    signature(device, &params, &samplers)
}

fn constants_param(count: u32) -> D3D12_ROOT_PARAMETER {
    let mut param = D3D12_ROOT_PARAMETER::default();
    param.ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    param.ShaderVisibility = D3D12_SHADER_VISIBILITY_ALL;
    param.Anonymous.Constants = D3D12_ROOT_CONSTANTS {
        ShaderRegister: 0,
        RegisterSpace: 0,
        Num32BitValues: count,
    };

    param
}

fn table_param(range: *const D3D12_DESCRIPTOR_RANGE) -> D3D12_ROOT_PARAMETER {
    let mut param = D3D12_ROOT_PARAMETER::default();
    param.ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    param.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    param.Anonymous.DescriptorTable = D3D12_ROOT_DESCRIPTOR_TABLE {
        NumDescriptorRanges: 1,
        pDescriptorRanges: range,
    };

    param
}

fn static_sampler() -> D3D12_STATIC_SAMPLER_DESC {
    D3D12_STATIC_SAMPLER_DESC {
        Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        MipLODBias: 0.0,
        MaxAnisotropy: 1,
        ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
        BorderColor: D3D12_STATIC_BORDER_COLOR_TRANSPARENT_BLACK,
        MinLOD: 0.0,
        MaxLOD: D3D12_FLOAT32_MAX,
        ShaderRegister: 0,
        RegisterSpace: 0,
        ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
    }
}

fn signature(device: &ID3D12Device, params: &[D3D12_ROOT_PARAMETER], samplers: &[D3D12_STATIC_SAMPLER_DESC]) -> Result<ID3D12RootSignature, String> {
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: params.len() as u32,
        pParameters: params.as_ptr(),
        NumStaticSamplers: samplers.len() as u32,
        pStaticSamplers: if samplers.is_empty() { std::ptr::null() } else { samplers.as_ptr() },
        Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
    };
    let mut blob = None;
    let mut errors = None;
    let serialized = unsafe { D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut errors)) };

    if let Err(err) = serialized {
        if let Some(errors) = errors {
            return Err(blob_text(&errors));
        }

        return Err(err.to_string());
    }

    let blob = blob.ok_or_else(|| "root signature".to_string())?;
    unsafe { device.CreateRootSignature(0, blob_bytes(&blob)).map_err(|err| err.to_string()) }
}

fn pipeline(
    device: &ID3D12Device,
    root: &ID3D12RootSignature,
    vs: &windows::Win32::Graphics::Direct3D::ID3DBlob,
    ps: &windows::Win32::Graphics::Direct3D::ID3DBlob,
    elements: &[D3D12_INPUT_ELEMENT_DESC],
    blend: bool,
    depth: bool,
    cull: D3D12_CULL_MODE,
) -> Result<ID3D12PipelineState, String> {
    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC::default();
    desc.pRootSignature = ManuallyDrop::new(Some(unsafe { std::mem::transmute_copy(root) }));
    desc.VS = shader_bytecode(vs);
    desc.PS = shader_bytecode(ps);
    desc.BlendState = blend_desc(blend);
    desc.SampleMask = u32::MAX;
    desc.RasterizerState = rasterizer(cull);
    desc.DepthStencilState = depth_desc(depth);
    desc.InputLayout = D3D12_INPUT_LAYOUT_DESC {
        pInputElementDescs: elements.as_ptr(),
        NumElements: elements.len() as u32,
    };
    desc.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    desc.NumRenderTargets = 1;
    desc.RTVFormats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.DSVFormat = DXGI_FORMAT_D32_FLOAT;
    desc.SampleDesc = DXGI_SAMPLE_DESC { Count: 1, Quality: 0 };
    unsafe { device.CreateGraphicsPipelineState(&desc).map_err(|err| err.to_string()) }
}

fn shader_bytecode(blob: &windows::Win32::Graphics::Direct3D::ID3DBlob) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: unsafe { blob.GetBufferPointer() },
        BytecodeLength: unsafe { blob.GetBufferSize() },
    }
}

fn element(name: PCSTR, format: DXGI_FORMAT, offset: u32) -> D3D12_INPUT_ELEMENT_DESC {
    D3D12_INPUT_ELEMENT_DESC {
        SemanticName: name,
        SemanticIndex: 0,
        Format: format,
        InputSlot: 0,
        AlignedByteOffset: offset,
        InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
        InstanceDataStepRate: 0,
    }
}

fn mesh_elements() -> [D3D12_INPUT_ELEMENT_DESC; 2] {
    [
        element(s!("POSITION"), DXGI_FORMAT_R32G32B32_FLOAT, 0),
        element(s!("COLOR"), DXGI_FORMAT_R32G32B32_FLOAT, 12),
    ]
}

fn color_elements() -> [D3D12_INPUT_ELEMENT_DESC; 2] {
    [
        element(s!("POSITION"), DXGI_FORMAT_R32G32_FLOAT, 0),
        element(s!("COLOR"), DXGI_FORMAT_R32G32B32A32_FLOAT, 8),
    ]
}

fn text_elements() -> [D3D12_INPUT_ELEMENT_DESC; 3] {
    [
        element(s!("POSITION"), DXGI_FORMAT_R32G32_FLOAT, 0),
        element(s!("TEXCOORD"), DXGI_FORMAT_R32G32_FLOAT, 8),
        element(s!("COLOR"), DXGI_FORMAT_R32G32B32A32_FLOAT, 16),
    ]
}

fn blend_desc(enable: bool) -> D3D12_BLEND_DESC {
    let mut desc = D3D12_BLEND_DESC::default();
    desc.RenderTarget[0] = D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: enable.into(),
        LogicOpEnable: false.into(),
        SrcBlend: D3D12_BLEND_SRC_ALPHA,
        DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D12_BLEND_OP_ADD,
        SrcBlendAlpha: D3D12_BLEND_ONE,
        DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D12_BLEND_OP_ADD,
        LogicOp: D3D12_LOGIC_OP_NOOP,
        RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };

    desc
}

fn rasterizer(cull: D3D12_CULL_MODE) -> D3D12_RASTERIZER_DESC {
    D3D12_RASTERIZER_DESC {
        FillMode: D3D12_FILL_MODE_SOLID,
        CullMode: cull,
        FrontCounterClockwise: true.into(),
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: true.into(),
        MultisampleEnable: false.into(),
        AntialiasedLineEnable: false.into(),
        ForcedSampleCount: 0,
        ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
    }
}

fn depth_desc(enable: bool) -> D3D12_DEPTH_STENCIL_DESC {
    let face = D3D12_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D12_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
        StencilPassOp: D3D12_STENCIL_OP_KEEP,
        StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
    };

    D3D12_DEPTH_STENCIL_DESC {
        DepthEnable: enable.into(),
        DepthWriteMask: if enable { D3D12_DEPTH_WRITE_MASK_ALL } else { D3D12_DEPTH_WRITE_MASK_ZERO },
        DepthFunc: if enable { D3D12_COMPARISON_FUNC_LESS } else { D3D12_COMPARISON_FUNC_ALWAYS },
        StencilEnable: false.into(),
        StencilReadMask: 0,
        StencilWriteMask: 0,
        FrontFace: face,
        BackFace: face,
    }
}
