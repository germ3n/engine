use crate::platform::Surface;
use crate::ui::shader;
use crate::ui::voxel::SceneView;
use crate::ui::vr::{self, EyeViews, Headset, VrInput};
use crate::ui::window::Window;
use crate::ui::Color;
use ash::vk::{self, Handle};
use glyph_brush::ab_glyph::FontArc;
use glyph_brush::{BrushAction, BrushError, Extra, GlyphBrush, GlyphBrushBuilder, Section, Text};
use std::ffi::{CStr, CString};

const FRAMES: usize = 2;

pub struct VulkanWindow {
    eyes: Option<Eyes>,
    swap: Swap,
    pipes: Pipelines,
    frames: Frames,
    mesh: HostBuffer,
    ui_bufs: [HostBuffer; FRAMES],
    text_bufs: [HostBuffer; FRAMES],
    atlas: Atlas,
    gpu: Gpu,
    width: u32,
    height: u32,
    ui: Vec<f32>,
    text: TextFrame,
    view: [f32; 24],
    clear: [f32; 4],
    mesh_vertices: u32,
    mesh_revision: u64,
    mesh_ready: bool,
    draw_mesh: bool,
    frame_idx: usize,
    vr: Option<Headset>,
    vr_failed: bool,
    vr_enable: bool,
    eye_views: Option<EyeViews>,
}

impl VulkanWindow {
    pub fn try_new(surface: &Surface) -> Result<Self, String> {
        let entry = unsafe { ash::Entry::load() }.map_err(|err| err.to_string())?;

        build_window(entry, surface)
    }
}

fn build_window(entry: ash::Entry, surface: &Surface) -> Result<VulkanWindow, String> {
    let gpu = Gpu::open(entry, surface)?;
    let format = surface_format(&gpu)?;
    let width = surface.width;
    let height = surface.height;
    let mut swap = Swap::create(&gpu, format, width, height)?;
    let pipes = Pipelines::create(&gpu, swap.format)?;
    swap.finish(&gpu, pipes.swap_pass)?;
    let frames = Frames::create(&gpu)?;
    let mesh = HostBuffer::create(&gpu, 4096, vk::BufferUsageFlags::VERTEX_BUFFER)?;
    let ui_bufs = [
        HostBuffer::create(&gpu, 4096, vk::BufferUsageFlags::VERTEX_BUFFER)?,
        HostBuffer::create(&gpu, 4096, vk::BufferUsageFlags::VERTEX_BUFFER)?,
    ];
    let text_bufs = [
        HostBuffer::create(&gpu, 4096, vk::BufferUsageFlags::VERTEX_BUFFER)?,
        HostBuffer::create(&gpu, 4096, vk::BufferUsageFlags::VERTEX_BUFFER)?,
    ];
    let text = TextFrame::new()?;
    let atlas = Atlas::create(&gpu, text.size.0, text.size.1)?;

    Ok(VulkanWindow {
        eyes: None,
        swap,
        pipes,
        frames,
        mesh,
        ui_bufs,
        text_bufs,
        atlas,
        gpu,
        width,
        height,
        ui: Vec::new(),
        text,
        view: [0.0; 24],
        clear: [0.0, 0.0, 0.0, 1.0],
        mesh_vertices: 0,
        mesh_revision: 0,
        mesh_ready: false,
        draw_mesh: false,
        frame_idx: 0,
        vr: None,
        vr_failed: false,
        vr_enable: false,
        eye_views: None,
    })
}

impl Drop for VulkanWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = self.gpu.device.device_wait_idle();
        }
        self.vr.take();
    }
}

impl Window for VulkanWindow {
    fn attach(surface: &Surface) -> Self {
        Self::try_new(surface).expect("vulkan window")
    }

    fn set_size(&mut self, w: u32, h: u32) {
        self.width = w.max(1);
        self.height = h.max(1);

        if let Err(err) = self
            .swap
            .resize(&self.gpu, self.width, self.height, self.pipes.swap_pass)
        {
            log::warn!("[gfx] vulkan resize {err}");
        }
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        self.clear = [red, green, blue, 1.0];
        self.ui.clear();
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
        self.eye_views = vr::connect(&mut self.vr, &mut self.vr_failed, self.vr_enable, view);
        let scene = match &self.eye_views {
            Some(eyes) => eyes.views[0],
            None => *view,
        };
        self.view = crate::world::surface::view_constants(
            vr::view_proj(&scene, true),
            scene.eye,
            scene.time,
            [self.width as f32, self.height as f32, 0.0, 0.0],
        );

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
        if self.swap.extent.width == 0 || self.swap.extent.height == 0 {
            return;
        }

        if let Err(err) = self.draw_frame() {
            log::warn!("[gfx] vulkan present {err}");
        }
    }
}

impl VulkanWindow {
    fn upload_mesh(&mut self, vertices: &[f32]) -> Result<(), String> {
        unsafe { self.gpu.device.device_wait_idle().map_err(vk_err)? };
        let bytes = bytes_of(vertices);
        self.mesh.ensure(&self.gpu, bytes.len() as u64)?;
        self.mesh.write(bytes);

        Ok(())
    }

    fn draw_frame(&mut self) -> Result<(), String> {
        let slot = self.frame_idx % FRAMES;
        self.frames.wait(slot)?;
        let acquired = self.swap.acquire(self.frames.acquire[slot]);

        let image = match acquired {
            Ok(image) => image,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.swap.resize(
                    &self.gpu,
                    self.width.max(1),
                    self.height.max(1),
                    self.pipes.swap_pass,
                )?;

                return Ok(());
            }
            Err(err) => return Err(vk_err(err)),
        };

        if self.text.dirty {
            unsafe { self.gpu.device.device_wait_idle().map_err(vk_err)? };
            self.atlas
                .resize(&self.gpu, self.text.size.0, self.text.size.1)?;
        }

        let ui_len = self.ui.len() * 4;
        let text_len = self.text.verts.len() * 4;
        self.ui_bufs[slot].ensure(&self.gpu, ui_len.max(4) as u64)?;
        self.text_bufs[slot].ensure(&self.gpu, text_len.max(4) as u64)?;
        self.ui_bufs[slot].write(bytes_of(&self.ui));
        self.text_bufs[slot].write(bytes_of(&self.text.verts));

        let eyes = self.eye_views;
        if let Some(frame) = eyes {
            self.ensure_eyes(frame.width, frame.height)?;
        }

        let cmd = self.frames.cmd[slot];
        unsafe {
            self.gpu
                .device
                .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                .map_err(vk_err)?;
            self.gpu
                .device
                .begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default())
                .map_err(vk_err)?;
        }

        if self.text.dirty {
            self.atlas.upload(cmd, &self.text.pixels)?;
            self.text.dirty = false;
        }

        if let Some(frame) = eyes {
            self.record_eyes(cmd, &frame)?;
        }

        self.record_window(cmd, image, ui_len, text_len)?;
        unsafe { self.gpu.device.end_command_buffer(cmd).map_err(vk_err)? };
        self.frames.submit(&self.gpu, slot, cmd)?;

        if let Some(frame) = eyes {
            self.submit_eyes(frame.width, frame.height);
        }

        let present = self.swap.present(&self.gpu, self.frames.ready[slot], image);
        self.frame_idx = self.frame_idx.wrapping_add(1);

        if let Some(headset) = self.vr.as_mut() {
            headset.handoff();
        }

        match present {
            Ok(()) => Ok(()),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                self.swap.resize(
                    &self.gpu,
                    self.width.max(1),
                    self.height.max(1),
                    self.pipes.swap_pass,
                )
            }
            Err(err) => Err(vk_err(err)),
        }
    }

    fn ensure_eyes(&mut self, width: u32, height: u32) -> Result<(), String> {
        if let Some(eyes) = &self.eyes {
            if eyes.width == width && eyes.height == height {
                return Ok(());
            }
        }

        unsafe { self.gpu.device.device_wait_idle().map_err(vk_err)? };
        self.eyes = Some(Eyes::create(&self.gpu, self.pipes.eye_pass, width, height)?);

        Ok(())
    }

    fn record_eyes(&mut self, cmd: vk::CommandBuffer, frame: &EyeViews) -> Result<(), String> {
        let Some(eyes) = &self.eyes else {
            return Ok(());
        };
        let mut idx = 0;

        while idx < 2 {
            let matrix = crate::world::surface::view_constants(
                vr::view_proj(&frame.views[idx], true),
                frame.views[idx].eye,
                frame.views[idx].time,
                [eyes.width as f32, eyes.height as f32, 0.0, 0.0],
            );
            let extent = vk::Extent2D {
                width: eyes.width,
                height: eyes.height,
            };
            begin_pass(
                &self.gpu.device,
                cmd,
                self.pipes.eye_pass,
                eyes.framebuffers[idx],
                extent,
                self.clear,
            );
            if self.draw_mesh {
                draw_mesh(
                    &self.gpu.device,
                    cmd,
                    self.pipes.mesh_layout,
                    self.pipes.mesh_eye,
                    self.mesh.buffer,
                    &matrix,
                    self.mesh_vertices,
                );
            }
            unsafe { self.gpu.device.cmd_end_render_pass(cmd) };
            idx += 1;
        }

        Ok(())
    }

    fn record_window(
        &self,
        cmd: vk::CommandBuffer,
        image: u32,
        ui_bytes: usize,
        text_bytes: usize,
    ) -> Result<(), String> {
        let extent = self.swap.extent;
        begin_pass(
            &self.gpu.device,
            cmd,
            self.pipes.swap_pass,
            self.swap.framebuffers[image as usize],
            extent,
            self.clear,
        );

        if self.draw_mesh {
            draw_mesh(
                &self.gpu.device,
                cmd,
                self.pipes.mesh_layout,
                self.pipes.mesh,
                self.mesh.buffer,
                &self.view,
                self.mesh_vertices,
            );
        }

        let mut screen = [0.0; 4];
        screen[0] = extent.width as f32;
        screen[1] = extent.height as f32;
        let ui_count = (ui_bytes / 24) as u32;

        if ui_count > 0 {
            draw_color(
                &self.gpu.device,
                cmd,
                self.pipes.color_layout,
                self.pipes.color,
                self.ui_bufs[self.frame_idx % FRAMES].buffer,
                &screen,
                ui_count,
            );
        }

        let text_count = (text_bytes / 32) as u32;

        if text_count > 0 {
            draw_text(
                &self.gpu.device,
                cmd,
                self.pipes.text_layout,
                self.pipes.text,
                self.text_bufs[self.frame_idx % FRAMES].buffer,
                self.atlas.set,
                &screen,
                text_count,
            );
        }

        unsafe { self.gpu.device.cmd_end_render_pass(cmd) };

        Ok(())
    }

    fn submit_eyes(&mut self, width: u32, height: u32) {
        let Some(eyes) = &self.eyes else {
            return;
        };
        let mut idx = 0;

        while idx < 2 {
            let mut data = VulkanEye {
                image: eyes.color[idx].image.as_raw(),
                device: self.gpu.device.handle().as_raw() as *mut std::ffi::c_void,
                physical_device: self.gpu.physical.as_raw() as *mut std::ffi::c_void,
                instance: self.gpu.instance.handle().as_raw() as *mut std::ffi::c_void,
                queue: self.gpu.queue.as_raw() as *mut std::ffi::c_void,
                queue_family: self.gpu.family,
                width,
                height,
                format: vk::Format::R8G8B8A8_UNORM.as_raw() as u32,
                samples: vk::SampleCountFlags::TYPE_1.as_raw(),
            };

            if let Some(headset) = self.vr.as_mut() {
                headset.submit_vulkan(idx, &mut data as *mut VulkanEye as *mut std::ffi::c_void);
            }

            idx += 1;
        }
    }
}

#[repr(C)]
struct VulkanEye {
    image: u64,
    device: *mut std::ffi::c_void,
    physical_device: *mut std::ffi::c_void,
    instance: *mut std::ffi::c_void,
    queue: *mut std::ffi::c_void,
    queue_family: u32,
    width: u32,
    height: u32,
    format: u32,
    samples: u32,
}

struct Gpu {
    #[allow(dead_code)]
    entry: ash::Entry,
    instance: ash::Instance,
    surface_loader: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    physical: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    family: u32,
    mem: vk::PhysicalDeviceMemoryProperties,
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}

impl Gpu {
    fn open(entry: ash::Entry, surface: &Surface) -> Result<Self, String> {
        let display = surface.display_handle_06();
        let window_handle = surface.window_handle_06();
        let mut names = Vec::new();
        let required = ash_window::enumerate_required_extensions(display).map_err(vk_err)?;

        for name in required {
            let text = unsafe { CStr::from_ptr(*name) }.to_string_lossy();
            push_name(&mut names, &text);
        }

        let supported =
            unsafe { entry.enumerate_instance_extension_properties(None) }.map_err(vk_err)?;
        let supported = extension_names(&supported);

        if supported
            .iter()
            .any(|name| name == "VK_KHR_portability_enumeration")
        {
            push_name(&mut names, "VK_KHR_portability_enumeration");
        }

        for extra in vr::vulkan_instance_extensions() {
            if supported.iter().any(|name| name == &extra) {
                push_name(&mut names, &extra);
            }
        }

        let ptrs = name_ptrs(&names);
        let mut flags = vk::InstanceCreateFlags::empty();

        if names
            .iter()
            .any(|name| name.to_bytes() == b"VK_KHR_portability_enumeration")
        {
            flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
        }

        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let info = vk::InstanceCreateInfo::default()
            .application_info(&app)
            .enabled_extension_names(&ptrs)
            .flags(flags);
        let instance = unsafe { entry.create_instance(&info, None) }.map_err(vk_err)?;
        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let surface =
            unsafe { ash_window::create_surface(&entry, &instance, display, window_handle, None) }
                .map_err(vk_err)?;
        let (physical, family) = pick_device(&instance, &surface_loader, surface)?;
        let device = create_device(&instance, physical, family)?;
        let queue = unsafe { device.get_device_queue(family, 0) };
        let mem = unsafe { instance.get_physical_device_memory_properties(physical) };

        Ok(Self {
            entry,
            instance,
            surface_loader,
            surface,
            physical,
            device,
            queue,
            family,
            mem,
        })
    }
}

struct Swap {
    device: ash::Device,
    loader: ash::khr::swapchain::Device,
    handle: vk::SwapchainKHR,
    format: vk::Format,
    extent: vk::Extent2D,
    views: Vec<vk::ImageView>,
    depth: GpuImage,
    framebuffers: Vec<vk::Framebuffer>,
}

impl Drop for Swap {
    fn drop(&mut self) {
        unsafe {
            for framebuffer in &self.framebuffers {
                self.device.destroy_framebuffer(*framebuffer, None);
            }

            for view in &self.views {
                self.device.destroy_image_view(*view, None);
            }

            self.loader.destroy_swapchain(self.handle, None);
        }
    }
}

impl Swap {
    fn create(
        gpu: &Gpu,
        format: (vk::Format, vk::ColorSpaceKHR),
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let loader = ash::khr::swapchain::Device::new(&gpu.instance, &gpu.device);
        let (handle, extent) = create_swapchain(
            gpu,
            &loader,
            vk::SwapchainKHR::null(),
            format,
            width,
            height,
        )?;
        let views = swap_views(&gpu.device, &loader, handle, format.0)?;
        let depth = gpu_image(
            gpu,
            extent.width,
            extent.height,
            vk::Format::D32_SFLOAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            vk::ImageAspectFlags::DEPTH,
        )?;

        Ok(Self {
            device: gpu.device.clone(),
            loader,
            handle,
            format: format.0,
            extent,
            views,
            depth,
            framebuffers: Vec::new(),
        })
    }

    fn finish(&mut self, gpu: &Gpu, pass: vk::RenderPass) -> Result<(), String> {
        self.framebuffers =
            framebuffers(&gpu.device, pass, &self.views, self.depth.view, self.extent)?;

        Ok(())
    }

    fn resize(
        &mut self,
        gpu: &Gpu,
        width: u32,
        height: u32,
        pass: vk::RenderPass,
    ) -> Result<(), String> {
        unsafe { gpu.device.device_wait_idle().map_err(vk_err)? };
        let format = (self.format, vk::ColorSpaceKHR::SRGB_NONLINEAR);
        let (handle, extent) =
            create_swapchain(gpu, &self.loader, self.handle, format, width, height)?;
        let views = swap_views(&gpu.device, &self.loader, handle, self.format)?;
        let depth = gpu_image(
            gpu,
            extent.width,
            extent.height,
            vk::Format::D32_SFLOAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            vk::ImageAspectFlags::DEPTH,
        )?;
        let buffers = framebuffers(&gpu.device, pass, &views, depth.view, extent)?;

        unsafe {
            for framebuffer in &self.framebuffers {
                self.device.destroy_framebuffer(*framebuffer, None);
            }

            for view in &self.views {
                self.device.destroy_image_view(*view, None);
            }

            self.loader.destroy_swapchain(self.handle, None);
        }

        self.handle = handle;
        self.extent = extent;
        self.views = views;
        self.depth = depth;
        self.framebuffers = buffers;

        Ok(())
    }

    fn acquire(&self, semaphore: vk::Semaphore) -> Result<u32, vk::Result> {
        let (index, _suboptimal) = unsafe {
            self.loader
                .acquire_next_image(self.handle, u64::MAX, semaphore, vk::Fence::null())?
        };

        Ok(index)
    }

    fn present(&self, gpu: &Gpu, semaphore: vk::Semaphore, image: u32) -> Result<(), vk::Result> {
        let wait = [semaphore];
        let swapchains = [self.handle];
        let indices = [image];
        let info = vk::PresentInfoKHR::default()
            .wait_semaphores(&wait)
            .swapchains(&swapchains)
            .image_indices(&indices);
        unsafe { self.loader.queue_present(gpu.queue, &info) }.map(|_| ())
    }
}

struct Eyes {
    device: ash::Device,
    color: [GpuImage; 2],
    #[allow(dead_code)]
    depth: [GpuImage; 2],
    framebuffers: [vk::Framebuffer; 2],
    width: u32,
    height: u32,
}

impl Drop for Eyes {
    fn drop(&mut self) {
        unsafe {
            for framebuffer in self.framebuffers {
                self.device.destroy_framebuffer(framebuffer, None);
            }
        }
    }
}

impl Eyes {
    fn create(gpu: &Gpu, pass: vk::RenderPass, width: u32, height: u32) -> Result<Self, String> {
        let width = width.max(1);
        let height = height.max(1);
        let usage = vk::ImageUsageFlags::COLOR_ATTACHMENT
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::SAMPLED;
        let color = [
            gpu_image(
                gpu,
                width,
                height,
                vk::Format::R8G8B8A8_UNORM,
                usage,
                vk::ImageAspectFlags::COLOR,
            )?,
            gpu_image(
                gpu,
                width,
                height,
                vk::Format::R8G8B8A8_UNORM,
                usage,
                vk::ImageAspectFlags::COLOR,
            )?,
        ];
        let depth = [
            gpu_image(
                gpu,
                width,
                height,
                vk::Format::D32_SFLOAT,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                vk::ImageAspectFlags::DEPTH,
            )?,
            gpu_image(
                gpu,
                width,
                height,
                vk::Format::D32_SFLOAT,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                vk::ImageAspectFlags::DEPTH,
            )?,
        ];
        let extent = vk::Extent2D { width, height };
        let left = framebuffers(&gpu.device, pass, &[color[0].view], depth[0].view, extent)?;
        let right = framebuffers(&gpu.device, pass, &[color[1].view], depth[1].view, extent)?;

        Ok(Self {
            device: gpu.device.clone(),
            color,
            depth,
            framebuffers: [left[0], right[0]],
            width,
            height,
        })
    }
}

struct Pipelines {
    device: ash::Device,
    swap_pass: vk::RenderPass,
    eye_pass: vk::RenderPass,
    mesh_layout: vk::PipelineLayout,
    color_layout: vk::PipelineLayout,
    text_layout: vk::PipelineLayout,
    mesh: vk::Pipeline,
    mesh_eye: vk::Pipeline,
    color: vk::Pipeline,
    text: vk::Pipeline,
}

impl Drop for Pipelines {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_pipeline(self.mesh, None);
            self.device.destroy_pipeline(self.mesh_eye, None);
            self.device.destroy_pipeline(self.color, None);
            self.device.destroy_pipeline(self.text, None);
            self.device.destroy_pipeline_layout(self.mesh_layout, None);
            self.device.destroy_pipeline_layout(self.color_layout, None);
            self.device.destroy_pipeline_layout(self.text_layout, None);
            self.device.destroy_render_pass(self.swap_pass, None);
            self.device.destroy_render_pass(self.eye_pass, None);
        }
    }
}

impl Pipelines {
    fn create(gpu: &Gpu, format: vk::Format) -> Result<Self, String> {
        let cache = shader::Registry::for_device(&device_uuid(&gpu.instance, gpu.physical));
        let swap_pass = render_pass(&gpu.device, format, vk::ImageLayout::PRESENT_SRC_KHR)?;
        let eye_pass = render_pass(
            &gpu.device,
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        )?;
        let mesh_layout = layout(
            &gpu.device,
            96,
            &[],
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
        )?;
        let color_layout = layout(&gpu.device, 16, &[], vk::ShaderStageFlags::VERTEX)?;
        let text_sets = [descriptor_layout(&gpu.device)?];
        let text_layout = layout(&gpu.device, 16, &text_sets, vk::ShaderStageFlags::VERTEX)?;
        let mesh_src = crate::ui::shaders::Program::Mesh.wgsl();
        let color_src = crate::ui::shaders::Program::Color.wgsl();
        let text_src = crate::ui::shaders::Program::Text.wgsl();
        let mesh_vert = cache.spirv(&mesh_src, naga::ShaderStage::Vertex, "vs_main")?;
        let mesh_frag = cache.spirv(&mesh_src, naga::ShaderStage::Fragment, "fs_main")?;
        let color_vert = cache.spirv(&color_src, naga::ShaderStage::Vertex, "vs_main")?;
        let color_frag = cache.spirv(&color_src, naga::ShaderStage::Fragment, "fs_main")?;
        let text_vert = cache.spirv(&text_src, naga::ShaderStage::Vertex, "vs_main")?;
        let text_frag = cache.spirv(&text_src, naga::ShaderStage::Fragment, "fs_main")?;
        let mesh_vs = shader_module(&gpu.device, &mesh_vert)?;
        let mesh_fs = shader_module(&gpu.device, &mesh_frag)?;
        let color_vs = shader_module(&gpu.device, &color_vert)?;
        let color_fs = shader_module(&gpu.device, &color_frag)?;
        let text_vs = shader_module(&gpu.device, &text_vert)?;
        let text_fs = shader_module(&gpu.device, &text_frag)?;
        let mesh = pipeline(
            &gpu.device,
            swap_pass,
            mesh_layout,
            mesh_vs,
            mesh_fs,
            c"vs_main",
            c"fs_main",
            &mesh_attrs(),
            (crate::world::STRIDE * 4) as u32,
            true,
            false,
            true,
        )?;
        let mesh_eye = pipeline(
            &gpu.device,
            eye_pass,
            mesh_layout,
            mesh_vs,
            mesh_fs,
            c"vs_main",
            c"fs_main",
            &mesh_attrs(),
            (crate::world::STRIDE * 4) as u32,
            true,
            false,
            true,
        )?;
        let color = pipeline(
            &gpu.device,
            swap_pass,
            color_layout,
            color_vs,
            color_fs,
            c"vs_main",
            c"fs_main",
            &color_attrs(),
            24,
            false,
            true,
            false,
        )?;
        let text = pipeline(
            &gpu.device,
            swap_pass,
            text_layout,
            text_vs,
            text_fs,
            c"vs_main",
            c"fs_main",
            &text_attrs(),
            32,
            false,
            true,
            false,
        )?;

        unsafe {
            gpu.device.destroy_shader_module(mesh_vs, None);
            gpu.device.destroy_shader_module(mesh_fs, None);
            gpu.device.destroy_shader_module(color_vs, None);
            gpu.device.destroy_shader_module(color_fs, None);
            gpu.device.destroy_shader_module(text_vs, None);
            gpu.device.destroy_shader_module(text_fs, None);
            gpu.device.destroy_descriptor_set_layout(text_sets[0], None);
        }

        Ok(Self {
            device: gpu.device.clone(),
            swap_pass,
            eye_pass,
            mesh_layout,
            color_layout,
            text_layout,
            mesh,
            mesh_eye,
            color,
            text,
        })
    }
}

struct Frames {
    device: ash::Device,
    pool: vk::CommandPool,
    cmd: [vk::CommandBuffer; FRAMES],
    fence: [vk::Fence; FRAMES],
    acquire: [vk::Semaphore; FRAMES],
    ready: [vk::Semaphore; FRAMES],
}

impl Drop for Frames {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_command_pool(self.pool, None);

            for idx in 0..FRAMES {
                self.device.destroy_fence(self.fence[idx], None);
                self.device.destroy_semaphore(self.acquire[idx], None);
                self.device.destroy_semaphore(self.ready[idx], None);
            }
        }
    }
}

impl Frames {
    fn create(gpu: &Gpu) -> Result<Self, String> {
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(gpu.family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let pool = unsafe { gpu.device.create_command_pool(&pool_info, None) }.map_err(vk_err)?;
        let alloc = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES as u32);
        let cmds = unsafe { gpu.device.allocate_command_buffers(&alloc) }.map_err(vk_err)?;
        let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let fence = [
            unsafe { gpu.device.create_fence(&fence_info, None) }.map_err(vk_err)?,
            unsafe { gpu.device.create_fence(&fence_info, None) }.map_err(vk_err)?,
        ];
        let sem_info = vk::SemaphoreCreateInfo::default();
        let acquire = [
            unsafe { gpu.device.create_semaphore(&sem_info, None) }.map_err(vk_err)?,
            unsafe { gpu.device.create_semaphore(&sem_info, None) }.map_err(vk_err)?,
        ];
        let ready = [
            unsafe { gpu.device.create_semaphore(&sem_info, None) }.map_err(vk_err)?,
            unsafe { gpu.device.create_semaphore(&sem_info, None) }.map_err(vk_err)?,
        ];

        Ok(Self {
            device: gpu.device.clone(),
            pool,
            cmd: [cmds[0], cmds[1]],
            fence,
            acquire,
            ready,
        })
    }

    fn wait(&self, slot: usize) -> Result<(), String> {
        unsafe {
            self.device
                .wait_for_fences(&[self.fence[slot]], true, u64::MAX)
                .map_err(vk_err)?
        };

        Ok(())
    }

    fn submit(&self, gpu: &Gpu, slot: usize, cmd: vk::CommandBuffer) -> Result<(), String> {
        unsafe {
            self.device
                .reset_fences(&[self.fence[slot]])
                .map_err(vk_err)?
        };
        let wait = [self.acquire[slot]];
        let signal = [self.ready[slot]];
        let cmds = [cmd];
        let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let info = vk::SubmitInfo::default()
            .wait_semaphores(&wait)
            .wait_dst_stage_mask(&stages)
            .command_buffers(&cmds)
            .signal_semaphores(&signal);
        unsafe {
            gpu.device
                .queue_submit(gpu.queue, &[info], self.fence[slot])
        }
        .map_err(vk_err)
    }
}

struct HostBuffer {
    device: ash::Device,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    ptr: *mut u8,
    capacity: u64,
}

impl Drop for HostBuffer {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_buffer(self.buffer, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

impl HostBuffer {
    fn create(gpu: &Gpu, size: u64, usage: vk::BufferUsageFlags) -> Result<Self, String> {
        let info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { gpu.device.create_buffer(&info, None) }.map_err(vk_err)?;
        let reqs = unsafe { gpu.device.get_buffer_memory_requirements(buffer) };
        let index = memory_index(
            &gpu.mem,
            reqs.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(reqs.size)
            .memory_type_index(index);
        let memory = unsafe { gpu.device.allocate_memory(&alloc, None) }.map_err(vk_err)?;
        unsafe {
            gpu.device
                .bind_buffer_memory(buffer, memory, 0)
                .map_err(vk_err)?
        };
        let ptr = unsafe {
            gpu.device
                .map_memory(memory, 0, reqs.size, vk::MemoryMapFlags::empty())
                .map_err(vk_err)?
        } as *mut u8;

        Ok(Self {
            device: gpu.device.clone(),
            buffer,
            memory,
            ptr,
            capacity: size,
        })
    }

    fn ensure(&mut self, gpu: &Gpu, bytes: u64) -> Result<(), String> {
        if bytes <= self.capacity {
            return Ok(());
        }

        *self = HostBuffer::create(
            gpu,
            grow(self.capacity, bytes),
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;

        Ok(())
    }

    fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() || self.ptr.is_null() {
            return;
        }

        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr, bytes.len()) };
    }
}

struct GpuImage {
    device: ash::Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
}

impl Drop for GpuImage {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_image_view(self.view, None);
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

struct Atlas {
    device: ash::Device,
    image: GpuImage,
    sampler: vk::Sampler,
    staging: HostBuffer,
    pool: vk::DescriptorPool,
    layout: vk::DescriptorSetLayout,
    set: vk::DescriptorSet,
    width: u32,
    height: u32,
    uploaded: bool,
}

impl Drop for Atlas {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_sampler(self.sampler, None);
            self.device.destroy_descriptor_pool(self.pool, None);
            self.device.destroy_descriptor_set_layout(self.layout, None);
        }
    }
}

impl Atlas {
    fn create(gpu: &Gpu, width: u32, height: u32) -> Result<Self, String> {
        let image = gpu_image(
            gpu,
            width,
            height,
            vk::Format::R8_UNORM,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
            vk::ImageAspectFlags::COLOR,
        )?;
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        let sampler = unsafe { gpu.device.create_sampler(&sampler_info, None) }.map_err(vk_err)?;
        let layout = descriptor_layout(&gpu.device)?;
        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLER)
                .descriptor_count(1),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(1)
            .pool_sizes(&pool_sizes);
        let pool =
            unsafe { gpu.device.create_descriptor_pool(&pool_info, None) }.map_err(vk_err)?;
        let layouts = [layout];
        let alloc = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts);
        let sets = unsafe { gpu.device.allocate_descriptor_sets(&alloc) }.map_err(vk_err)?;
        let staging = HostBuffer::create(
            gpu,
            (width as u64) * (height as u64),
            vk::BufferUsageFlags::TRANSFER_SRC,
        )?;
        let atlas = Self {
            device: gpu.device.clone(),
            image,
            sampler,
            staging,
            pool,
            layout,
            set: sets[0],
            width,
            height,
            uploaded: false,
        };
        atlas.write_set();

        Ok(atlas)
    }

    fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) -> Result<(), String> {
        if self.width == width && self.height == height {
            return Ok(());
        }

        self.image = gpu_image(
            gpu,
            width,
            height,
            vk::Format::R8_UNORM,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
            vk::ImageAspectFlags::COLOR,
        )?;
        self.staging = HostBuffer::create(
            gpu,
            (width as u64) * (height as u64),
            vk::BufferUsageFlags::TRANSFER_SRC,
        )?;
        self.width = width;
        self.height = height;
        self.uploaded = false;
        self.write_set();

        Ok(())
    }

    fn write_set(&self) {
        let image_info = vk::DescriptorImageInfo::default()
            .image_view(self.image.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        let sampler_info = vk::DescriptorImageInfo::default().sampler(self.sampler);
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(self.set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(std::slice::from_ref(&image_info)),
            vk::WriteDescriptorSet::default()
                .dst_set(self.set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .image_info(std::slice::from_ref(&sampler_info)),
        ];
        unsafe { self.device.update_descriptor_sets(&writes, &[]) };
    }

    fn upload(&mut self, cmd: vk::CommandBuffer, pixels: &[u8]) -> Result<(), String> {
        let bytes = (self.width as usize) * (self.height as usize);
        self.staging.write(&pixels[..bytes.min(pixels.len())]);
        let range = color_range();
        let old_layout = if self.uploaded {
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
        } else {
            vk::ImageLayout::UNDEFINED
        };
        let src_stage = if self.uploaded {
            vk::PipelineStageFlags::FRAGMENT_SHADER
        } else {
            vk::PipelineStageFlags::TOP_OF_PIPE
        };
        let src_access = if self.uploaded {
            vk::AccessFlags::SHADER_READ
        } else {
            vk::AccessFlags::empty()
        };
        self.uploaded = true;
        let to_copy = vk::ImageMemoryBarrier::default()
            .old_layout(old_layout)
            .src_access_mask(src_access)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.image.image)
            .subresource_range(range)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
        unsafe {
            self.device.cmd_pipeline_barrier(
                cmd,
                src_stage,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_copy],
            );
        }
        let region = vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_extent(vk::Extent3D {
                width: self.width,
                height: self.height,
                depth: 1,
            });
        unsafe {
            self.device.cmd_copy_buffer_to_image(
                cmd,
                self.staging.buffer,
                self.image.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
        }
        let to_read = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.image.image)
            .subresource_range(range)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ);
        unsafe {
            self.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_read],
            );
        }

        Ok(())
    }
}

fn begin_pass(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    extent: vk::Extent2D,
    clear: [f32; 4],
) {
    let clears = [
        vk::ClearValue {
            color: vk::ClearColorValue { float32: clear },
        },
        vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
                depth: 1.0,
                stencil: 0,
            },
        },
    ];
    let info = vk::RenderPassBeginInfo::default()
        .render_pass(pass)
        .framebuffer(framebuffer)
        .render_area(vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        })
        .clear_values(&clears);
    let viewport = vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: extent.width as f32,
        height: extent.height as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    };
    let scissor = vk::Rect2D {
        offset: vk::Offset2D::default(),
        extent,
    };
    unsafe {
        device.cmd_begin_render_pass(cmd, &info, vk::SubpassContents::INLINE);
        device.cmd_set_viewport(cmd, 0, &[viewport]);
        device.cmd_set_scissor(cmd, 0, &[scissor]);
    }
}

fn draw_mesh(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    buffer: vk::Buffer,
    matrix: &[f32; 24],
    count: u32,
) {
    unsafe {
        device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
        device.cmd_push_constants(
            cmd,
            layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            bytes_of(matrix),
        );
        device.cmd_bind_vertex_buffers(cmd, 0, &[buffer], &[0]);
        device.cmd_draw(cmd, count, 1, 0, 0);
    }
}

fn draw_color(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    buffer: vk::Buffer,
    screen: &[f32; 4],
    count: u32,
) {
    unsafe {
        device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
        device.cmd_push_constants(
            cmd,
            layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            bytes_of(screen),
        );
        device.cmd_bind_vertex_buffers(cmd, 0, &[buffer], &[0]);
        device.cmd_draw(cmd, count, 1, 0, 0);
    }
}

fn draw_text(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    buffer: vk::Buffer,
    set: vk::DescriptorSet,
    screen: &[f32; 4],
    count: u32,
) {
    unsafe {
        device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
        device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            layout,
            0,
            &[set],
            &[],
        );
        device.cmd_push_constants(
            cmd,
            layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            bytes_of(screen),
        );
        device.cmd_bind_vertex_buffers(cmd, 0, &[buffer], &[0]);
        device.cmd_draw(cmd, count, 1, 0, 0);
    }
}

fn create_device(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    family: u32,
) -> Result<ash::Device, String> {
    let priority = 1.0f32;
    let queue = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(std::slice::from_ref(&priority));
    let mut names = vec![ash::khr::swapchain::NAME.to_owned()];
    let supported =
        unsafe { instance.enumerate_device_extension_properties(physical) }.map_err(vk_err)?;
    let supported = extension_names(&supported);

    if supported
        .iter()
        .any(|name| name == "VK_KHR_portability_subset")
    {
        push_name(&mut names, "VK_KHR_portability_subset");
    }

    for extra in vr::vulkan_device_extensions(physical.as_raw() as *mut std::ffi::c_void) {
        if supported.iter().any(|name| name == &extra) {
            push_name(&mut names, &extra);
        }
    }

    let ptrs = name_ptrs(&names);
    let info = vk::DeviceCreateInfo::default()
        .queue_create_infos(std::slice::from_ref(&queue))
        .enabled_extension_names(&ptrs);

    unsafe { instance.create_device(physical, &info, None) }.map_err(vk_err)
}

fn device_uuid(instance: &ash::Instance, physical: vk::PhysicalDevice) -> String {
    let mut id = vk::PhysicalDeviceIDProperties::default();
    let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
    unsafe {
        instance.get_physical_device_properties2(physical, &mut props);
    }

    shader::id_from_bytes(&id.device_uuid)
}

fn pick_device(
    instance: &ash::Instance,
    surface: &ash::khr::surface::Instance,
    target: vk::SurfaceKHR,
) -> Result<(vk::PhysicalDevice, u32), String> {
    let devices = unsafe { instance.enumerate_physical_devices() }.map_err(vk_err)?;
    let mut best: Option<(vk::PhysicalDevice, u32, u32)> = None;

    for device in devices {
        let Some(family) = queue_family(instance, surface, device, target) else {
            continue;
        };
        let props = unsafe { instance.get_physical_device_properties(device) };
        let score = if props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU {
            2
        } else {
            1
        };

        if best.map(|(_, _, have)| score > have).unwrap_or(true) {
            best = Some((device, family, score));
        }
    }

    let Some((device, family, _)) = best else {
        return Err("vulkan device".to_string());
    };

    Ok((device, family))
}

fn queue_family(
    instance: &ash::Instance,
    surface: &ash::khr::surface::Instance,
    device: vk::PhysicalDevice,
    target: vk::SurfaceKHR,
) -> Option<u32> {
    let families = unsafe { instance.get_physical_device_queue_family_properties(device) };
    let mut idx = 0;

    while idx < families.len() {
        let graphics = families[idx].queue_flags.contains(vk::QueueFlags::GRAPHICS);
        let present =
            unsafe { surface.get_physical_device_surface_support(device, idx as u32, target) }
                .unwrap_or(false);

        if graphics && present {
            return Some(idx as u32);
        }

        idx += 1;
    }

    None
}

fn surface_format(gpu: &Gpu) -> Result<(vk::Format, vk::ColorSpaceKHR), String> {
    let formats = unsafe {
        gpu.surface_loader
            .get_physical_device_surface_formats(gpu.physical, gpu.surface)
    }
    .map_err(vk_err)?;

    for format in &formats {
        if format.format == vk::Format::R8G8B8A8_UNORM
            && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        {
            return Ok((format.format, format.color_space));
        }
    }

    for format in &formats {
        if format.format == vk::Format::B8G8R8A8_UNORM
            && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        {
            return Ok((format.format, format.color_space));
        }
    }

    let Some(format) = formats.first() else {
        return Err("vulkan surface format".to_string());
    };

    Ok((format.format, format.color_space))
}

fn create_swapchain(
    gpu: &Gpu,
    loader: &ash::khr::swapchain::Device,
    old: vk::SwapchainKHR,
    format: (vk::Format, vk::ColorSpaceKHR),
    width: u32,
    height: u32,
) -> Result<(vk::SwapchainKHR, vk::Extent2D), String> {
    let caps = unsafe {
        gpu.surface_loader
            .get_physical_device_surface_capabilities(gpu.physical, gpu.surface)
    }
    .map_err(vk_err)?;
    let modes = unsafe {
        gpu.surface_loader
            .get_physical_device_surface_present_modes(gpu.physical, gpu.surface)
    }
    .map_err(vk_err)?;
    let extent = if caps.current_extent.width != u32::MAX {
        caps.current_extent
    } else {
        vk::Extent2D {
            width: width.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
            height: height.clamp(caps.min_image_extent.height, caps.max_image_extent.height),
        }
    };
    let mut count = caps.min_image_count + 1;

    if caps.max_image_count > 0 && count > caps.max_image_count {
        count = caps.max_image_count;
    }

    let info = vk::SwapchainCreateInfoKHR::default()
        .surface(gpu.surface)
        .min_image_count(count)
        .image_format(format.0)
        .image_color_space(format.1)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .pre_transform(caps.current_transform)
        .composite_alpha(composite_alpha(caps.supported_composite_alpha))
        .present_mode(present_mode(&modes))
        .clipped(true)
        .old_swapchain(old);
    let handle = unsafe { loader.create_swapchain(&info, None) }.map_err(vk_err)?;

    Ok((handle, extent))
}

fn composite_alpha(flags: vk::CompositeAlphaFlagsKHR) -> vk::CompositeAlphaFlagsKHR {
    let order = [
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::INHERIT,
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
    ];

    for flag in order {
        if flags.contains(flag) {
            return flag;
        }
    }

    vk::CompositeAlphaFlagsKHR::OPAQUE
}

fn present_mode(modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    if modes.contains(&vk::PresentModeKHR::IMMEDIATE) {
        return vk::PresentModeKHR::IMMEDIATE;
    }

    if modes.contains(&vk::PresentModeKHR::MAILBOX) {
        return vk::PresentModeKHR::MAILBOX;
    }

    vk::PresentModeKHR::FIFO
}

fn swap_views(
    device: &ash::Device,
    loader: &ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    format: vk::Format,
) -> Result<Vec<vk::ImageView>, String> {
    let images = unsafe { loader.get_swapchain_images(swapchain) }.map_err(vk_err)?;
    let mut views = Vec::new();

    for image in images {
        views.push(image_view(
            device,
            image,
            format,
            vk::ImageAspectFlags::COLOR,
        )?);
    }

    Ok(views)
}

fn framebuffers(
    device: &ash::Device,
    pass: vk::RenderPass,
    colors: &[vk::ImageView],
    depth: vk::ImageView,
    extent: vk::Extent2D,
) -> Result<Vec<vk::Framebuffer>, String> {
    let mut buffers = Vec::new();

    for color in colors {
        let attachments = [*color, depth];
        let info = vk::FramebufferCreateInfo::default()
            .render_pass(pass)
            .attachments(&attachments)
            .width(extent.width)
            .height(extent.height)
            .layers(1);
        buffers.push(unsafe { device.create_framebuffer(&info, None) }.map_err(vk_err)?);
    }

    Ok(buffers)
}

fn render_pass(
    device: &ash::Device,
    format: vk::Format,
    final_layout: vk::ImageLayout,
) -> Result<vk::RenderPass, String> {
    let color = vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(final_layout);
    let depth = vk::AttachmentDescription::default()
        .format(vk::Format::D32_SFLOAT)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::DONT_CARE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
    let attachments = [color, depth];
    let color_ref = vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
    let depth_ref = vk::AttachmentReference::default()
        .attachment(1)
        .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
    let subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(std::slice::from_ref(&color_ref))
        .depth_stencil_attachment(&depth_ref);
    let dependency = vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
        )
        .dst_stage_mask(
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
        )
        .dst_access_mask(
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        );
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(std::slice::from_ref(&subpass))
        .dependencies(std::slice::from_ref(&dependency));

    unsafe { device.create_render_pass(&info, None) }.map_err(vk_err)
}

fn layout(
    device: &ash::Device,
    push_size: u32,
    sets: &[vk::DescriptorSetLayout],
    stages: vk::ShaderStageFlags,
) -> Result<vk::PipelineLayout, String> {
    let push = vk::PushConstantRange::default()
        .stage_flags(stages)
        .offset(0)
        .size(push_size);
    let info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(sets)
        .push_constant_ranges(std::slice::from_ref(&push));

    unsafe { device.create_pipeline_layout(&info, None) }.map_err(vk_err)
}

fn descriptor_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout, String> {
    let bindings = [
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
    ];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);

    unsafe { device.create_descriptor_set_layout(&info, None) }.map_err(vk_err)
}

fn pipeline(
    device: &ash::Device,
    pass: vk::RenderPass,
    layout: vk::PipelineLayout,
    vert: vk::ShaderModule,
    frag: vk::ShaderModule,
    vert_entry: &CStr,
    frag_entry: &CStr,
    attrs: &[vk::VertexInputAttributeDescription],
    stride: u32,
    depth: bool,
    blend: bool,
    cull: bool,
) -> Result<vk::Pipeline, String> {
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vert)
            .name(vert_entry),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(frag)
            .name(frag_entry),
    ];
    let binding = vk::VertexInputBindingDescription::default()
        .binding(0)
        .stride(stride)
        .input_rate(vk::VertexInputRate::VERTEX);
    let vertex = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(std::slice::from_ref(&binding))
        .vertex_attribute_descriptions(attrs);
    let input = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(if cull {
            vk::CullModeFlags::BACK
        } else {
            vk::CullModeFlags::NONE
        })
        .front_face(if cfg!(target_os = "macos") {
            vk::FrontFace::COUNTER_CLOCKWISE
        } else {
            vk::FrontFace::CLOCKWISE
        })
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let depth_state = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(depth)
        .depth_write_enable(depth)
        .depth_compare_op(vk::CompareOp::LESS);
    let blend_state = if blend {
        vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA)
    } else {
        vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(false)
            .color_write_mask(vk::ColorComponentFlags::RGBA)
    };
    let blend_info = vk::PipelineColorBlendStateCreateInfo::default()
        .attachments(std::slice::from_ref(&blend_state));
    let dynamics = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamics);
    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex)
        .input_assembly_state(&input)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .depth_stencil_state(&depth_state)
        .color_blend_state(&blend_info)
        .dynamic_state(&dynamic)
        .layout(layout)
        .render_pass(pass)
        .subpass(0);

    match unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) } {
        Ok(mut pipelines) => Ok(pipelines.remove(0)),
        Err((_, err)) => Err(vk_err(err)),
    }
}

fn shader_module(device: &ash::Device, words: &[u32]) -> Result<vk::ShaderModule, String> {
    let info = vk::ShaderModuleCreateInfo::default().code(words);

    unsafe { device.create_shader_module(&info, None) }.map_err(vk_err)
}

fn gpu_image(
    gpu: &Gpu,
    width: u32,
    height: u32,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    aspect: vk::ImageAspectFlags,
) -> Result<GpuImage, String> {
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(format)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    let image = unsafe { gpu.device.create_image(&info, None) }.map_err(vk_err)?;
    let reqs = unsafe { gpu.device.get_image_memory_requirements(image) };
    let index = memory_index(
        &gpu.mem,
        reqs.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )
    .or_else(|_| {
        memory_index(
            &gpu.mem,
            reqs.memory_type_bits,
            vk::MemoryPropertyFlags::empty(),
        )
    })?;
    let alloc = vk::MemoryAllocateInfo::default()
        .allocation_size(reqs.size)
        .memory_type_index(index);
    let memory = unsafe { gpu.device.allocate_memory(&alloc, None) }.map_err(vk_err)?;
    unsafe {
        gpu.device
            .bind_image_memory(image, memory, 0)
            .map_err(vk_err)?
    };
    let view = image_view(&gpu.device, image, format, aspect)?;

    Ok(GpuImage {
        device: gpu.device.clone(),
        image,
        memory,
        view,
    })
}

fn image_view(
    device: &ash::Device,
    image: vk::Image,
    format: vk::Format,
    aspect: vk::ImageAspectFlags,
) -> Result<vk::ImageView, String> {
    let info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: aspect,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });

    unsafe { device.create_image_view(&info, None) }.map_err(vk_err)
}

fn memory_index(
    mem: &vk::PhysicalDeviceMemoryProperties,
    bits: u32,
    flags: vk::MemoryPropertyFlags,
) -> Result<u32, String> {
    let mut idx = 0;

    while idx < mem.memory_type_count {
        let kind = (bits & (1 << idx)) != 0;
        let props = mem.memory_types[idx as usize]
            .property_flags
            .contains(flags);

        if kind && props {
            return Ok(idx);
        }

        idx += 1;
    }

    Err("vulkan memory type".to_string())
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

fn extension_names(exts: &[vk::ExtensionProperties]) -> Vec<String> {
    exts.iter()
        .map(|ext| {
            let bytes = ext.extension_name.map(|ch| ch as u8);
            let end = bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(bytes.len());
            String::from_utf8_lossy(&bytes[..end]).into_owned()
        })
        .collect()
}

fn push_name(list: &mut Vec<CString>, name: &str) {
    if list.iter().any(|have| have.to_str().ok() == Some(name)) {
        return;
    }

    if let Ok(value) = CString::new(name) {
        list.push(value);
    }
}

fn name_ptrs(list: &[CString]) -> Vec<*const std::ffi::c_char> {
    list.iter().map(|name| name.as_ptr()).collect()
}

fn vk_err(err: impl std::fmt::Debug) -> String {
    format!("vulkan {err:?}")
}

fn bytes_of(values: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr() as *const u8, values.len() * 4) }
}

fn grow(current: u64, needed: u64) -> u64 {
    let mut size = current.max(4096);

    while size < needed {
        size = size.saturating_mul(2);
    }

    size
}

fn mesh_attrs() -> [vk::VertexInputAttributeDescription; 8] {
    let attr = |location, format, offset| {
        vk::VertexInputAttributeDescription::default()
            .location(location)
            .binding(0)
            .format(format)
            .offset(offset)
    };

    [
        attr(0, vk::Format::R32G32B32_SFLOAT, 0),
        attr(1, vk::Format::R32G32B32_SFLOAT, 12),
        attr(2, vk::Format::R32G32B32A32_SFLOAT, 24),
        attr(3, vk::Format::R32G32_SFLOAT, 40),
        attr(4, vk::Format::R32G32_SFLOAT, 48),
        attr(5, vk::Format::R32G32B32_SFLOAT, 56),
        attr(6, vk::Format::R32_SFLOAT, 68),
        attr(7, vk::Format::R32_SFLOAT, 72),
    ]
}

fn color_attrs() -> [vk::VertexInputAttributeDescription; 2] {
    [
        vk::VertexInputAttributeDescription::default()
            .location(0)
            .binding(0)
            .format(vk::Format::R32G32_SFLOAT)
            .offset(0),
        vk::VertexInputAttributeDescription::default()
            .location(1)
            .binding(0)
            .format(vk::Format::R32G32B32A32_SFLOAT)
            .offset(8),
    ]
}

fn text_attrs() -> [vk::VertexInputAttributeDescription; 3] {
    [
        vk::VertexInputAttributeDescription::default()
            .location(0)
            .binding(0)
            .format(vk::Format::R32G32_SFLOAT)
            .offset(0),
        vk::VertexInputAttributeDescription::default()
            .location(1)
            .binding(0)
            .format(vk::Format::R32G32_SFLOAT)
            .offset(8),
        vk::VertexInputAttributeDescription::default()
            .location(2)
            .binding(0)
            .format(vk::Format::R32G32B32A32_SFLOAT)
            .offset(16),
    ]
}

fn push_rect(verts: &mut Vec<f32>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    let corners = [
        [x, y],
        [x + w, y],
        [x, y + h],
        [x, y + h],
        [x + w, y],
        [x + w, y + h],
    ];

    for corner in corners {
        verts.push(corner[0]);
        verts.push(corner[1]);
        verts.extend_from_slice(&color);
    }
}

fn push_outline(
    verts: &mut Vec<f32>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    thickness: f32,
    color: [f32; 4],
) {
    push_rect(verts, x, y, w, thickness, color);
    push_rect(verts, x, y + h - thickness, w, thickness, color);
    push_rect(
        verts,
        x,
        y + thickness,
        thickness,
        h - 2.0 * thickness,
        color,
    );
    push_rect(
        verts,
        x + w - thickness,
        y + thickness,
        thickness,
        h - 2.0 * thickness,
        color,
    );
}

#[derive(Clone, Copy)]
struct GlyphQuad {
    verts: [[f32; 8]; 6],
}

struct TextFrame {
    glyphs: GlyphBrush<GlyphQuad>,
    verts: Vec<f32>,
    pixels: Vec<u8>,
    size: (u32, u32),
    dirty: bool,
}

impl TextFrame {
    fn new() -> Result<Self, String> {
        let font = FontArc::try_from_slice(include_bytes!("font_default.ttf"))
            .map_err(|err| err.to_string())?;
        let glyphs = GlyphBrushBuilder::using_font(font)
            .initial_cache_size((512, 512))
            .build();

        Ok(Self {
            glyphs,
            verts: Vec::new(),
            pixels: vec![0; 512 * 512],
            size: (512, 512),
            dirty: true,
        })
    }

    fn queue(&mut self, text: &str, x: f32, y: f32, scale: f32, color: [f32; 4]) {
        self.glyphs.queue(
            Section::default()
                .add_text(Text::new(text).with_scale(scale).with_color(color))
                .with_screen_position((x, y)),
        );
    }

    fn build(&mut self) {
        for _attempt in 0..4 {
            let result = self.glyphs.process_queued(
                |rect, data| {
                    let width = (rect.max[0] - rect.min[0]) as usize;
                    let height = (rect.max[1] - rect.min[1]) as usize;

                    if width == 0 || height == 0 {
                        return;
                    }

                    let mut row = 0;

                    while row < height {
                        let dst = (rect.min[1] as usize + row) * self.size.0 as usize
                            + rect.min[0] as usize;
                        let src = row * width;
                        self.pixels[dst..dst + width].copy_from_slice(&data[src..src + width]);
                        row += 1;
                    }

                    self.dirty = true;
                },
                glyph_quad,
            );

            match result {
                Ok(BrushAction::Draw(quads)) => {
                    self.verts.clear();

                    for quad in quads {
                        for idx in 0..6 {
                            self.verts.extend_from_slice(&quad.verts[idx]);
                        }
                    }

                    return;
                }
                Ok(BrushAction::ReDraw) => {
                    return;
                }
                Err(BrushError::TextureTooSmall { suggested }) => {
                    self.glyphs.resize_texture(suggested.0, suggested.1);
                    self.pixels = vec![0; suggested.0 as usize * suggested.1 as usize];
                    self.size = suggested;
                    self.dirty = true;
                }
            }
        }
    }
}

fn glyph_quad(vertex: glyph_brush::GlyphVertex<Extra>) -> GlyphQuad {
    let color = vertex.extra.color;
    let positions = [
        [
            vertex.pixel_coords.min.x,
            vertex.pixel_coords.min.y,
            vertex.tex_coords.min.x,
            vertex.tex_coords.min.y,
        ],
        [
            vertex.pixel_coords.max.x,
            vertex.pixel_coords.min.y,
            vertex.tex_coords.max.x,
            vertex.tex_coords.min.y,
        ],
        [
            vertex.pixel_coords.min.x,
            vertex.pixel_coords.max.y,
            vertex.tex_coords.min.x,
            vertex.tex_coords.max.y,
        ],
        [
            vertex.pixel_coords.min.x,
            vertex.pixel_coords.max.y,
            vertex.tex_coords.min.x,
            vertex.tex_coords.max.y,
        ],
        [
            vertex.pixel_coords.max.x,
            vertex.pixel_coords.min.y,
            vertex.tex_coords.max.x,
            vertex.tex_coords.min.y,
        ],
        [
            vertex.pixel_coords.max.x,
            vertex.pixel_coords.max.y,
            vertex.tex_coords.max.x,
            vertex.tex_coords.max.y,
        ],
    ];
    let mut verts = [[0.0; 8]; 6];

    for idx in 0..6 {
        verts[idx] = [
            positions[idx][0],
            positions[idx][1],
            positions[idx][2],
            positions[idx][3],
            color[0],
            color[1],
            color[2],
            color[3],
        ];
    }

    GlyphQuad { verts }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::voxel::FlyCamera;
    use crate::world::{Block, BlockPos, VoxelWorld};

    #[test]
    fn vulkan_shaders_compile() {
        let mesh_src = crate::ui::shaders::Program::Mesh.wgsl();
        let color_src = crate::ui::shaders::Program::Color.wgsl();
        let text_src = crate::ui::shaders::Program::Text.wgsl();
        let mesh = shader::spirv(&mesh_src, naga::ShaderStage::Vertex, "vs_main").unwrap();
        let color = shader::spirv(&color_src, naga::ShaderStage::Vertex, "vs_main").unwrap();
        let text = shader::spirv(&text_src, naga::ShaderStage::Fragment, "fs_main").unwrap();
        assert_eq!(mesh[0], 0x07230203);
        assert_eq!(color[0], 0x07230203);
        assert_eq!(text[0], 0x07230203);
        shader::spirv(&mesh_src, naga::ShaderStage::Fragment, "fs_main").unwrap();
        shader::spirv(&color_src, naga::ShaderStage::Fragment, "fs_main").unwrap();
        shader::spirv(&text_src, naga::ShaderStage::Vertex, "vs_main").unwrap();
    }

    #[test]
    fn vulkan_projection_shows_front_faces() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        let mesh = world.mesh();
        let view = FlyCamera::new().scene(4.0 / 3.0, 1.0);
        let view_proj = vr::view_proj(&view, true);
        let mut visible = 0;
        let mut front = 0;
        let mut idx = 0;

        while idx + crate::world::STRIDE <= mesh.len() {
            let a = project(&view_proj, [mesh[idx], mesh[idx + 1], mesh[idx + 2]]);
            let b = project(&view_proj, [mesh[idx + 6], mesh[idx + 7], mesh[idx + 8]]);
            let c = project(&view_proj, [mesh[idx + 12], mesh[idx + 13], mesh[idx + 14]]);

            if a[3] > 0.0 && b[3] > 0.0 && c[3] > 0.0 {
                let ax = a[0] / a[3];
                let ay = a[1] / a[3];
                let az = a[2] / a[3];
                let bx = b[0] / b[3];
                let by = b[1] / b[3];
                let bz = b[2] / b[3];
                let cx = c[0] / c[3];
                let cy = c[1] / c[3];
                let cz = c[2] / c[3];
                let on_screen = ax.abs() < 1.5
                    && ay.abs() < 1.5
                    && bx.abs() < 1.5
                    && by.abs() < 1.5
                    && cx.abs() < 1.5
                    && cy.abs() < 1.5;
                let in_depth = az > 0.0 && az < 1.0 && bz > 0.0 && bz < 1.0 && cz > 0.0 && cz < 1.0;

                if on_screen && in_depth {
                    visible += 1;
                    let winding = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);

                    if winding > 0.0 {
                        front += 1;
                    }
                }
            }

            idx += crate::world::STRIDE;
        }

        assert!(visible > 0);
        assert!(front > 0);
    }

    fn project(view_proj: &[f32; 16], position: [f32; 3]) -> [f32; 4] {
        let p = [position[0], position[1], position[2], 1.0];
        let mut clip = [0.0; 4];
        let mut row = 0;

        while row < 4 {
            let mut sum = 0.0;
            let mut col = 0;

            while col < 4 {
                sum += view_proj[col * 4 + row] * p[col];
                col += 1;
            }

            clip[row] = sum;
            row += 1;
        }

        clip
    }
}
