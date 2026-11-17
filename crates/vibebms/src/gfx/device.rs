//! The GPU: device and surface set-up, resizing, the present mode, and
//! drawing a frame's batch.

use super::batch::Vertex;
use super::texture::GpuTexture;
use super::{Batch, Color, SdfStyle, ATLAS, ATLAS_SIZE};
use crate::platform::{Duration, Instant};
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub struct Gfx {
    pub surface: wgpu::Surface<'static>,
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
    pub(super) tex_layout: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    pub(super) sampler: wgpu::Sampler,
    pub(super) textures: Vec<Option<GpuTexture>>,
    vbuf: wgpu::Buffer,
    vcap: u64,
    pub present_modes: Vec<wgpu::PresentMode>,
    pub adapter_name: String,
    /// Told before each present (Wayland paces frames by it).
    window: Arc<winit::window::Window>,
}

impl Gfx {
    /// Blocking constructor for native platforms.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        window: Arc<winit::window::Window>,
        mode: crate::config::PresentMode,
    ) -> Result<Gfx, String> {
        pollster::block_on(Self::create(window, mode))
    }

    /// Async constructor (required in browsers). Uses WebGPU when available
    /// and falls back to WebGL2.
    pub async fn create(
        window: Arc<winit::window::Window>,
        mode: crate::config::PresentMode,
    ) -> Result<Gfx, String> {
        let desc = wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        };
        let instance = if web_gl_only() {
            wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::GL,
                ..desc
            })
        } else {
            wgpu::util::new_instance_with_webgpu_detection(&desc).await
        };
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("no suitable GPU adapter")?;
        let info = adapter.get_info();
        log::info!("gpu: {} ({:?})", info.name, info.backend);
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("vibebms"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                        .using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| e.to_string())?;
        // a bad draw or texture is logged, not fatal
        device.on_uncaptured_error(Box::new(|e| log::error!("gpu: {e}")));
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
        };
        surface.configure(&device, &config);

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
            bind_group_layouts: &[&uniform_layout, &tex_layout],
            push_constant_ranges: &[],
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
            bind_group_layouts: &[&uniform_layout, &tex_layout, &sdf_layout],
            push_constant_ranges: &[],
        });
        let make_with = |blend: wgpu::BlendState, sdf: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if sdf { "sdf" } else { "2d" }),
            layout: Some(if sdf { &sdf_pipeline_layout } else { &layout }),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
                }],
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
            multiview: None,
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
        let mut g = Gfx {
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
            tex_layout,
            uniform_buf,
            uniform_bind,
            sampler,
            textures: Vec::new(),
            vbuf,
            vcap,
            present_modes: caps.present_modes.clone(),
            adapter_name: format!("{} ({:?})", info.name, info.backend),
            window,
        };
        // atlas: transparent with a white block in the corner
        let mut px = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize];
        for y in 0..4 {
            for x in 0..4 {
                let i = ((y * ATLAS_SIZE + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let id = g.create_texture(ATLAS_SIZE, ATLAS_SIZE, &px);
        debug_assert_eq!(id, ATLAS);
        Ok(g)
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
        self.surface.configure(&self.device, &self.config);
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
        self.surface.configure(&self.device, &self.config);
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

    /// Draw and present a frame; how long it waited for its image (with
    /// FIFO, the display holding it back: Vulkan, DX12 and Metal wait
    /// there).
    pub fn render(&mut self, batch: &Batch, clear: Color) -> Result<Duration, wgpu::SurfaceError> {
        let t0 = Instant::now();
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated) | Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(Duration::ZERO);
            }
            Err(e) => return Err(e),
        };
        let waited = t0.elapsed();
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
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            });
            pass.set_pipeline(&self.pipelines[0]);
            pass.set_bind_group(0, &self.uniform_bind, &[]);
            pass.set_vertex_buffer(0, self.vbuf.slice(..));
            let (sw, sh) = (self.config.width, self.config.height);
            for c in &batch.cmds {
                let Some(Some(t)) = self.textures.get(c.tex) else {
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
        self.queue.submit(Some(enc.finish()));
        // winit: on Wayland the next redraw request follows the compositor's
        // frame callback only when told a frame is about to be shown
        self.window.pre_present_notify();
        frame.present();
        Ok(waited)
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
