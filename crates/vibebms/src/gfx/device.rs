//! The GPU: device and surface set-up, resizing, the present mode, and
//! drawing a frame's batch.

use super::batch::Vertex;
use super::texture::{TexDevice, Texture};
use super::{Batch, Color, Gfx, SdfStyle};
use crate::config::GraphicsBackend;
use crate::platform::{Duration, Instant};
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// What draws and presents frames; the textures they are drawn from are
/// a [`Gfx`] made with it.
pub struct Gpu {
    /// The window's (`None`: headless, drawn into a texture of
    /// `offscreen`).
    pub surface: Option<wgpu::Surface<'static>>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pipelines: [wgpu::RenderPipeline; 5],
    /// The same for distance-field glyphs, with their styles at
    /// `sdf_stride` apart in `sdf_buf` (a dynamic offset each).
    sdf_pipelines: [wgpu::RenderPipeline; 5],
    sdf_layout: wgpu::BindGroupLayout,
    sdf_buf: wgpu::Buffer,
    sdf_bind: wgpu::BindGroup,
    sdf_stride: u64,
    sdf_cap: u64,
    uniform_buf: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    vbuf: wgpu::Buffer,
    vcap: u64,
    pub present_modes: Vec<wgpu::PresentMode>,
    /// The surface's images can be copied from (screenshots).
    can_copy: bool,
    /// A screenshot of the next frame is wanted; one being read.
    capture_next: bool,
    capture: Option<super::capture::Pending>,
    pub adapter_name: String,
    /// Set when the graphics API chosen did not start (`Gpu::new`).
    pub fallback: Option<Fallback>,
    /// Told before each present (Wayland paces frames by it).
    window: Option<Arc<winit::window::Window>>,
}

/// The graphics API asked for could not start and another one did
/// (`Gpu::new`): the app says so once and keeps the one that started.
#[derive(Clone, Debug, PartialEq)]
pub struct Fallback {
    pub wanted: GraphicsBackend,
    pub used: GraphicsBackend,
    pub error: String,
}

impl Gpu {
    /// Blocking constructor for native platforms: the graphics API chosen
    /// (`backend`), else the next one the platform has that starts
    /// (`attempts`); `fallback` then says which.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        window: Arc<winit::window::Window>,
        mode: crate::config::PresentMode,
        backend: GraphicsBackend,
    ) -> Result<(Gpu, Gfx), String> {
        let mut errors: Vec<(GraphicsBackend, String)> = Vec::new();
        for b in attempts(backend, &available_backends()) {
            match pollster::block_on(Self::create_with(window.clone(), mode, wgpu_backends(b))) {
                Ok((mut g, gfx)) => {
                    if let Some((_, e)) = errors.first() {
                        g.fallback = Some(Fallback {
                            wanted: backend,
                            used: b,
                            error: e.clone(),
                        });
                    }
                    return Ok((g, gfx));
                }
                Err(e) => {
                    log::warn!("gpu: {} could not start: {e}", b.label());
                    errors.push((b, e));
                }
            }
        }
        Err(errors
            .iter()
            .map(|(b, e)| format!("{}: {e}", b.label()))
            .collect::<Vec<_>>()
            .join("; "))
    }

    /// Async constructor (required in browsers). Uses WebGPU when available
    /// and falls back to WebGL2.
    pub async fn create(
        window: Arc<winit::window::Window>,
        mode: crate::config::PresentMode,
    ) -> Result<(Gpu, Gfx), String> {
        Self::create_with(window, mode, wgpu::Backends::all()).await
    }

    async fn create_with(
        window: Arc<winit::window::Window>,
        mode: crate::config::PresentMode,
        backends: wgpu::Backends,
    ) -> Result<(Gpu, Gfx), String> {
        // (OpenGL on Linux presents through the window's display connection)
        let display: Box<dyn wgpu::wgt::WgpuHasDisplayHandle> = Box::new(window.clone());
        let desc = instance_desc(backends, Some(display));
        let instance = if web_gl_only() {
            wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::GL,
                ..desc
            })
        } else {
            wgpu::util::new_instance_with_webgpu_detection(desc).await
        };
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|_| "no suitable GPU adapter")?;
        let info = adapter.get_info();
        log::info!("gpu: {} ({:?})", info.name, info.backend);
        let (device, queue) = adapter
            .request_device(&device_desc("vibebms", &adapter))
            .await
            .map_err(|e| e.to_string())?;
        // a bad draw or texture is logged, not fatal
        device.on_uncaptured_error(Arc::new(|e| log::error!("gpu: {e}")));
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = crate::platform::screen::drawable_size(&window);
        let present_mode = choose_present_mode(&caps.present_modes, mode);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: frame_latency(present_mode),
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);
        Ok(Self::with_device(
            device,
            queue,
            config,
            &caps,
            &info,
            Some((surface, window)),
        ))
    }

    /// The pipelines and buffers for drawing on `device` into images of
    /// `config` (`out`: the window's surface; without it, headless), and
    /// the textures (with the atlas) made on it.
    pub(super) fn with_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        caps: &wgpu::SurfaceCapabilities,
        info: &wgpu::AdapterInfo,
        out: Option<(wgpu::Surface<'static>, Arc<winit::window::Window>)>,
    ) -> (Gpu, Gfx) {
        let (surface, window) = out.unzip();
        let format = config.format;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("2d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniforms"),
            contents: bytemuck::cast_slice(&[config.width as f32, config.height as f32, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("2d"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let sdf_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sdf"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<SdfStyle>() as u64),
                },
                count: None,
            }],
        });
        let sdf_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sdf"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&tex_layout), Some(&sdf_layout)],
            immediate_size: 0,
        });
        let make_with = |blend: wgpu::BlendState, sdf: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if sdf { "sdf" } else { "2d" }),
            layout: Some(if sdf { &sdf_pipeline_layout } else { &layout }),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if sdf { "fs_sdf" } else { "fs_main" }),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
        };
        let make = |blend| make_with(blend, false);
        let color = |src, dst, op| wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: src,
                dst_factor: dst,
                operation: op,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        use wgpu::{BlendFactor as F, BlendOperation as O};
        // indexed by `Blend`
        let blends = [
            wgpu::BlendState::ALPHA_BLENDING,
            color(F::SrcAlpha, F::One, O::Add),
            color(F::SrcAlpha, F::One, O::ReverseSubtract),
            color(F::Zero, F::Src, O::Add),
            color(F::OneMinusDst, F::Zero, O::Add),
        ];
        let pipelines = blends.map(make);
        let sdf_pipelines = blends.map(|b| make_with(b, true));
        let sdf_stride = (std::mem::size_of::<SdfStyle>() as u64)
            .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment as u64);
        let sdf_cap = 16;
        let (sdf_buf, sdf_bind) = sdf_buffer(&device, &sdf_layout, sdf_stride * sdf_cap);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let vcap = 65536u64;
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("verts"),
            size: vcap * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gfx = Gfx::new(TexDevice {
            device: device.clone(),
            queue: queue.clone(),
            layout: tex_layout,
            sampler,
        });
        let g = Gpu {
            surface,
            device,
            queue,
            config,
            pipelines,
            sdf_pipelines,
            sdf_layout,
            sdf_buf,
            sdf_bind,
            sdf_stride,
            sdf_cap,
            uniform_buf,
            uniform_bind,
            vbuf,
            vcap,
            present_modes: caps.present_modes.clone(),
            can_copy: caps.usages.contains(wgpu::TextureUsages::COPY_SRC),
            capture_next: false,
            capture: None,
            adapter_name: format!("{} ({:?})", info.name, info.backend),
            fallback: None,
            window,
        };
        (g, gfx)
    }

    pub fn width(&self) -> f32 {
        self.config.width as f32
    }
    pub fn height(&self) -> f32 {
        self.config.height as f32
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        }
        self.queue.write_buffer(
            &self.uniform_buf,
            0,
            bytemuck::cast_slice(&[w as f32, h as f32, 0.0, 0.0]),
        );
    }

    pub fn set_present_mode(&mut self, mode: crate::config::PresentMode) {
        let m = choose_present_mode(&self.present_modes, mode);
        self.config.present_mode = m;
        self.config.desired_maximum_frame_latency = frame_latency(m);
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        }
    }

    /// Copy the next frame for a screenshot ([`Self::take_capture`]);
    /// `false` where the surface's images cannot be copied or read. The
    /// surface is made copyable the first time (not before: it may cost
    /// some GPUs' compression of it).
    pub fn request_capture(&mut self) -> bool {
        let Some(surface) = self.surface.as_ref() else {
            return false;
        };
        if !self.can_copy || !super::capture::readable(self.config.format) {
            return false;
        }
        let copy = wgpu::TextureUsages::COPY_SRC;
        if !self.config.usage.contains(copy) {
            self.config.usage |= copy;
            surface.configure(&self.device, &self.config);
        }
        self.capture_next = true;
        true
    }

    /// The screenshot's image once it has been read back (without waiting
    /// for the GPU: some frames after it was asked for).
    pub fn take_capture(&mut self) -> Option<Result<::image::RgbaImage, String>> {
        let r = self.capture.as_ref()?;
        let _ = self.device.poll(wgpu::PollType::Poll);
        let img = r.take()?;
        self.capture = None;
        Some(img)
    }

    /// The display paces the frames (each waits for a vblank).
    pub fn vsync(&self) -> bool {
        self.config.present_mode == wgpu::PresentMode::Fifo
    }

    /// With vsync, how many periods after it starts a frame is on the
    /// screen, about: the frames queued before it.
    pub fn frames_ahead(&self) -> Option<f64> {
        self.vsync()
            .then_some(self.config.desired_maximum_frame_latency as f64)
    }

    /// Draw and present a frame (from `gfx`'s textures); how long it
    /// waited for its image (with FIFO, the display holding it back:
    /// Vulkan, DX12 and Metal wait there). An image that could not be had
    /// is an error, its frame not drawn (a covered window's, on macOS:
    /// none, quietly).
    pub fn render(
        &mut self,
        gfx: &Gfx,
        batch: &Batch,
        clear: Color,
    ) -> Result<Duration, &'static str> {
        let t0 = Instant::now();
        let Some(surface) = self.surface.as_ref() else {
            return Ok(Duration::ZERO);
        };
        use wgpu::CurrentSurfaceTexture as C;
        let frame = match surface.get_current_texture() {
            C::Success(f) | C::Suboptimal(f) => f,
            C::Outdated | C::Lost => {
                surface.configure(&self.device, &self.config);
                return Ok(Duration::ZERO);
            }
            C::Occluded => return Ok(Duration::ZERO),
            C::Timeout => return Err("timed out waiting for the window's next image"),
            C::Validation => return Err("the window's next image could not be had"),
        };
        let waited = t0.elapsed();
        let mut enc = self.encode(gfx, batch, clear, &frame.texture);
        let capture = match std::mem::take(&mut self.capture_next) {
            true if self.capture.is_none() => {
                super::capture::copy(&self.device, &mut enc, &frame.texture)
            }
            _ => None,
        };
        self.queue.submit(Some(enc.finish()));
        if let Some(c) = capture {
            c.start();
            self.capture = Some(c);
        }
        // winit: on Wayland the next redraw request follows the compositor's
        // frame callback only when told a frame is about to be shown
        if let Some(w) = &self.window {
            w.pre_present_notify();
        }
        self.queue.present(frame);
        Ok(waited)
    }

    /// A frame's batch uploaded and its drawing into `target` (of the
    /// config's size and format) recorded.
    pub(super) fn encode(
        &mut self,
        gfx: &Gfx,
        batch: &Batch,
        clear: Color,
        target: &wgpu::Texture,
    ) -> wgpu::CommandEncoder {
        let n = batch.verts.len() as u64;
        if n > self.vcap {
            self.vcap = n.next_power_of_two();
            self.vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("verts"),
                size: self.vcap * std::mem::size_of::<Vertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if n > 0 {
            self.queue
                .write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&batch.verts));
        }
        let styles = batch.styles.len() as u64;
        if styles > self.sdf_cap {
            self.sdf_cap = styles.next_power_of_two();
            (self.sdf_buf, self.sdf_bind) = sdf_buffer(
                &self.device,
                &self.sdf_layout,
                self.sdf_stride * self.sdf_cap,
            );
        }
        if styles > 0 {
            let stride = self.sdf_stride as usize;
            let mut bytes = vec![0u8; stride * batch.styles.len()];
            for (i, st) in batch.styles.iter().enumerate() {
                let b = bytemuck::bytes_of(st);
                bytes[i * stride..i * stride + b.len()].copy_from_slice(b);
            }
            self.queue.write_buffer(&self.sdf_buf, 0, &bytes);
        }
        let view = target.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0] as f64,
                            g: clear[1] as f64,
                            b: clear[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines[0]);
            pass.set_bind_group(0, &self.uniform_bind, &[]);
            pass.set_vertex_buffer(0, self.vbuf.slice(..));
            let (sw, sh) = (self.config.width, self.config.height);
            for c in &batch.cmds {
                let Some(Some(Texture { gpu: Some(t), .. })) = gfx.textures.get(c.tex) else {
                    continue;
                };
                match c.clip {
                    Some([x, y, w, h]) => {
                        let x = x.min(sw);
                        let y = y.min(sh);
                        let w = w.min(sw - x);
                        let h = h.min(sh - y);
                        if w == 0 || h == 0 {
                            continue;
                        }
                        pass.set_scissor_rect(x, y, w, h);
                    }
                    None => pass.set_scissor_rect(0, 0, sw, sh),
                }
                match c.sdf {
                    Some(i) => {
                        pass.set_pipeline(&self.sdf_pipelines[c.blend as usize]);
                        let off = (i as u64 * self.sdf_stride) as u32;
                        pass.set_bind_group(2, &self.sdf_bind, &[off]);
                    }
                    None => pass.set_pipeline(&self.pipelines[c.blend as usize]),
                }
                pass.set_bind_group(1, &t.bind, &[]);
                pass.draw(c.start..c.start + c.count, 0..1);
            }
        }
        enc
    }
}

/// The uniform buffer of distance-field styles, `size` bytes.
fn sdf_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    size: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sdf styles"),
        size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sdf styles"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buf,
                offset: 0,
                size: wgpu::BufferSize::new(std::mem::size_of::<SdfStyle>() as u64),
            }),
        }],
    });
    (buf, bind)
}

/// An instance of `backends` (`display`: the window's, which OpenGL on
/// Linux presents through), with wgpu's defaults but for DX12's shader
/// compiler: FXC, as ever (wgpu's default is now DXC where its DLL is
/// found).
pub(super) fn instance_desc(
    backends: wgpu::Backends,
    display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>,
) -> wgpu::InstanceDescriptor {
    let mut desc = match display {
        Some(d) => wgpu::InstanceDescriptor::new_with_display_handle(d),
        None => wgpu::InstanceDescriptor::new_without_display_handle(),
    };
    desc.backends = backends;
    desc.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
    desc
}

/// The device asked of `adapter`: no optional features, WebGL2's limits
/// but for the adapter's largest textures.
pub(super) fn device_desc<'a>(
    label: &'a str,
    adapter: &wgpu::Adapter,
) -> wgpu::DeviceDescriptor<'a> {
    wgpu::DeviceDescriptor {
        label: Some(label),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_webgl2_defaults()
            .using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }
}

/// The wgpu backends of a setting (AUTO: all, and wgpu picks).
pub fn wgpu_backends(b: GraphicsBackend) -> wgpu::Backends {
    match b {
        GraphicsBackend::Auto => wgpu::Backends::all(),
        GraphicsBackend::Vulkan => wgpu::Backends::VULKAN,
        GraphicsBackend::Metal => wgpu::Backends::METAL,
        GraphicsBackend::Dx12 => wgpu::Backends::DX12,
        GraphicsBackend::OpenGl => wgpu::Backends::GL,
    }
}

/// The graphics APIs this build has on this platform (Windows: Vulkan,
/// DX12, OpenGL; macOS and iOS: Metal; Linux and Android: Vulkan, OpenGL),
/// in AUTO's order; the settings offer these.
pub fn available_backends() -> Vec<GraphicsBackend> {
    available_in(wgpu::Instance::enabled_backend_features())
}

fn available_in(enabled: wgpu::Backends) -> Vec<GraphicsBackend> {
    GraphicsBackend::APIS
        .into_iter()
        .filter(|&b| enabled.contains(wgpu_backends(b)))
        .collect()
}

/// The settings tried in turn for `chosen`: it first, then each API of
/// `avail` not tried yet, in AUTO's order. A choice this platform does not
/// have (a configuration from another computer) is AUTO.
#[cfg(not(target_arch = "wasm32"))]
fn attempts(chosen: GraphicsBackend, avail: &[GraphicsBackend]) -> Vec<GraphicsBackend> {
    let first = if chosen.is_auto() || avail.contains(&chosen) {
        chosen
    } else {
        GraphicsBackend::Auto
    };
    let mut v = vec![first];
    v.extend(avail.iter().copied().filter(|&b| b != first));
    v
}

/// In a browser: draw with WebGL2 even where WebGPU is offered, when the
/// page's address says `?gpu=webgl` (to compare the two).
fn web_gl_only() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.location().search().ok())
            .is_some_and(|q| q.contains("gpu=webgl"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    false
}

/// The present mode for a setting (see [`crate::config::PresentMode`]):
/// tearing only when asked for (Unlimited), and FIFO (which every GPU has)
/// where the one wanted is missing.
fn choose_present_mode(
    avail: &[wgpu::PresentMode],
    mode: crate::config::PresentMode,
) -> wgpu::PresentMode {
    use crate::config::PresentMode as P;
    use wgpu::PresentMode as W;
    let pick = |order: &[W]| {
        order
            .iter()
            .copied()
            .find(|m| avail.contains(m))
            .unwrap_or(W::Fifo)
    };
    match mode {
        P::VSync => W::Fifo,
        P::LowLatency => pick(&[W::Mailbox]),
        P::Immediate => pick(&[W::Immediate, W::Mailbox]),
    }
}

/// Frames queued for the display: with FIFO two (three images), so that
/// one slow frame does not miss its vblank; else one, the newest frame
/// replacing a queued one.
fn frame_latency(mode: wgpu::PresentMode) -> u32 {
    match mode {
        wgpu::PresentMode::Fifo => 2,
        _ => 1,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    /// The shader parses and validates (with naga, as wgpu checks it when
    /// it makes the pipelines), distance-field stage included.
    #[test]
    fn shader_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(include_str!("shader.wgsl"))
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(include_str!("shader.wgsl"))));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("the shader validates");
        for name in ["vs_main", "fs_main", "fs_sdf"] {
            assert!(module.entry_points.iter().any(|e| e.name == name), "{name}");
        }
    }

    /// The chosen API first, then the platform's others in AUTO's order;
    /// one the platform lacks is AUTO.
    #[test]
    fn backend_attempts() {
        use super::{attempts, available_in};
        use crate::config::GraphicsBackend::*;
        let windows =
            available_in(wgpu::Backends::VULKAN | wgpu::Backends::DX12 | wgpu::Backends::GL);
        assert_eq!(windows, [Vulkan, Dx12, OpenGl]);
        assert_eq!(available_in(wgpu::Backends::METAL), [Metal]);
        assert_eq!(attempts(Auto, &windows), [Auto, Vulkan, Dx12, OpenGl]);
        assert_eq!(attempts(Dx12, &windows), [Dx12, Vulkan, OpenGl]);
        assert_eq!(attempts(OpenGl, &windows), [OpenGl, Vulkan, Dx12]);
        assert_eq!(attempts(Metal, &windows), [Auto, Vulkan, Dx12, OpenGl]);
        assert_eq!(attempts(Metal, &[Metal]), [Metal]);
    }

    /// Tearing only when asked for; FIFO, two frames queued, where the mode
    /// wanted is missing (Metal has FIFO and, on macOS, immediate).
    #[test]
    fn present_modes() {
        use super::{choose_present_mode, frame_latency};
        use crate::config::PresentMode as P;
        use wgpu::PresentMode as W;
        let metal = [W::Fifo, W::Immediate];
        let vulkan = [W::Fifo, W::FifoRelaxed, W::Mailbox, W::Immediate];
        for (avail, mode, want) in [
            (&metal[..], P::VSync, W::Fifo),
            (&metal[..], P::LowLatency, W::Fifo),
            (&metal[..], P::Immediate, W::Immediate),
            (&vulkan[..], P::VSync, W::Fifo),
            (&vulkan[..], P::LowLatency, W::Mailbox),
            (&vulkan[..], P::Immediate, W::Immediate),
            (&[W::Fifo, W::Mailbox][..], P::Immediate, W::Mailbox),
        ] {
            assert_eq!(
                choose_present_mode(avail, mode),
                want,
                "{mode:?} of {avail:?}"
            );
        }
        assert_eq!(frame_latency(W::Fifo), 2);
        assert_eq!(frame_latency(W::Mailbox), 1);
    }
}
