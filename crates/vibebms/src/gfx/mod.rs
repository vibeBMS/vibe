//! Minimal, fast 2D renderer on wgpu: one textured-quad pipeline, batched by
//! texture. Works on Vulkan / Metal / DX12 / GL(ES).

pub mod text;

use bytemuck::{Pod, Zeroable};
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub type TexId = usize;
/// Texture 0 is the glyph atlas; its top-left 4x4 pixels are opaque white.
pub const ATLAS: TexId = 0;
pub const WHITE_UV: [f32; 4] = [0.5 / 2048.0, 0.5 / 2048.0, 1.5 / 2048.0, 1.5 / 2048.0];
pub const ATLAS_SIZE: u32 = 2048;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

pub type Color = [f32; 4];

pub fn rgba(hex: u32) -> Color {
    [
        ((hex >> 24) & 0xff) as f32 / 255.0,
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
    ]
}

pub fn with_alpha(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], c[3] * a]
}

/// How a draw combines with what is below (skin blend modes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Blend {
    #[default]
    Alpha,
    Add,
    /// below − source
    Subtract,
    /// below × source
    Multiply,
    /// inverts what is below
    Invert,
}

impl Blend {
    /// LR2 / beatoraja `blend` value (1 = alpha, 2 add, 3 subtract,
    /// 4 multiply, 9 invert).
    pub fn from_skin(v: i32) -> Blend {
        match v {
            2 => Blend::Add,
            3 => Blend::Subtract,
            4 => Blend::Multiply,
            9 => Blend::Invert,
            _ => Blend::Alpha,
        }
    }
}

struct Cmd {
    tex: TexId,
    start: u32,
    count: u32,
    clip: Option<[u32; 4]>,
    blend: Blend,
}

/// Geometry for one frame.
#[derive(Default)]
pub struct Batch {
    verts: Vec<Vertex>,
    cmds: Vec<Cmd>,
    clip: Option<[u32; 4]>,
    /// Outer clip every `set_clip` stays inside (a skin's viewport).
    base_clip: Option<Rect>,
    blend: Blend,
}

impl Batch {
    pub fn clear(&mut self) {
        self.verts.clear();
        self.cmds.clear();
        self.clip = None;
        self.base_clip = None;
        self.blend = Blend::Alpha;
    }

    /// Additive blending for following draws (skin glow effects).
    pub fn set_additive(&mut self, on: bool) {
        self.blend = if on { Blend::Add } else { Blend::Alpha };
    }

    /// Blend mode for following draws.
    pub fn set_blend(&mut self, b: Blend) {
        self.blend = b;
    }

    fn push_cmd(&mut self, tex: TexId, count: u32) {
        let start = self.verts.len() as u32;
        match self.cmds.last_mut() {
            Some(c)
                if c.tex == tex
                    && c.clip == self.clip
                    && c.blend == self.blend
                    && c.start + c.count == start =>
            {
                c.count += count
            }
            _ => self.cmds.push(Cmd {
                tex,
                start,
                count,
                clip: self.clip,
                blend: self.blend,
            }),
        }
    }

    /// Textured quad rotated by `angle` degrees around `pivot` (fractions of the rect).
    pub fn quad_rot(
        &mut self,
        tex: TexId,
        r: Rect,
        uv: [f32; 4],
        color: Color,
        angle: f32,
        pivot: (f32, f32),
    ) {
        if angle == 0.0 {
            return self.quad(tex, r, uv, color);
        }
        if r.w <= 0.0 || r.h <= 0.0 || color[3] <= 0.0 {
            return;
        }
        self.push_cmd(tex, 6);
        let (cx, cy) = (r.x + r.w * pivot.0, r.y + r.h * pivot.1);
        let (sn, cs) = angle.to_radians().sin_cos();
        let v = |x: f32, y: f32, u: f32, w: f32| {
            let (dx, dy) = (x - cx, y - cy);
            Vertex {
                pos: [cx + dx * cs - dy * sn, cy + dx * sn + dy * cs],
                uv: [u, w],
                color,
            }
        };
        let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
        self.verts.extend_from_slice(&[
            v(x0, y0, uv[0], uv[1]),
            v(x1, y0, uv[2], uv[1]),
            v(x0, y1, uv[0], uv[3]),
            v(x1, y0, uv[2], uv[1]),
            v(x1, y1, uv[2], uv[3]),
            v(x0, y1, uv[0], uv[3]),
        ]);
    }

    /// Clip following draws to `r` (within the base clip); `None` = the
    /// base clip only.
    pub fn set_clip(&mut self, r: Option<Rect>) {
        let r = match (r, self.base_clip) {
            (Some(r), Some(b)) => {
                let (x0, y0) = (r.x.max(b.x), r.y.max(b.y));
                let (x1, y1) = (r.right().min(b.right()), r.bottom().min(b.bottom()));
                Some(Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)))
            }
            (r, b) => r.or(b),
        };
        self.clip = r.map(|r| {
            [
                r.x.max(0.0) as u32,
                r.y.max(0.0) as u32,
                r.w.max(0.0) as u32,
                r.h.max(0.0) as u32,
            ]
        });
    }

    /// Set (or remove) the outer clip; resets the current clip to it.
    pub fn set_base_clip(&mut self, r: Option<Rect>) {
        self.base_clip = r;
        self.set_clip(None);
    }

    pub fn quad(&mut self, tex: TexId, r: Rect, uv: [f32; 4], color: Color) {
        if r.w <= 0.0 || r.h <= 0.0 || color[3] <= 0.0 {
            return;
        }
        self.push_cmd(tex, 6);
        let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
        let v = |x, y, u, w| Vertex {
            pos: [x, y],
            uv: [u, w],
            color,
        };
        self.verts.extend_from_slice(&[
            v(x0, y0, uv[0], uv[1]),
            v(x1, y0, uv[2], uv[1]),
            v(x0, y1, uv[0], uv[3]),
            v(x1, y0, uv[2], uv[1]),
            v(x1, y1, uv[2], uv[3]),
            v(x0, y1, uv[0], uv[3]),
        ]);
    }

    pub fn rect(&mut self, r: Rect, color: Color) {
        self.quad(ATLAS, r, WHITE_UV, color);
    }

    pub fn rect_outline(&mut self, r: Rect, t: f32, color: Color) {
        self.rect(Rect::new(r.x, r.y, r.w, t), color);
        self.rect(Rect::new(r.x, r.bottom() - t, r.w, t), color);
        self.rect(Rect::new(r.x, r.y + t, t, r.h - 2.0 * t), color);
        self.rect(Rect::new(r.right() - t, r.y + t, t, r.h - 2.0 * t), color);
    }

    /// Vertical gradient rectangle.
    pub fn gradient(&mut self, r: Rect, top: Color, bottom: Color) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        self.push_cmd(ATLAS, 6);
        let uv = WHITE_UV;
        let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
        let v = |x, y, c| Vertex {
            pos: [x, y],
            uv: [uv[0], uv[1]],
            color: c,
        };
        self.verts.extend_from_slice(&[
            v(x0, y0, top),
            v(x1, y0, top),
            v(x0, y1, bottom),
            v(x1, y0, top),
            v(x1, y1, bottom),
            v(x0, y1, bottom),
        ]);
    }

    /// Filled rectangle with rounded corners.
    pub fn rounded_rect(&mut self, r: Rect, radius: f32, color: Color) {
        let rad = radius.min(r.w * 0.5).min(r.h * 0.5);
        if rad < 0.75 || r.w <= 0.0 || r.h <= 0.0 {
            return self.rect(r, color);
        }
        self.rect(Rect::new(r.x + rad, r.y, r.w - 2.0 * rad, r.h), color);
        self.rect(Rect::new(r.x, r.y + rad, rad, r.h - 2.0 * rad), color);
        self.rect(
            Rect::new(r.right() - rad, r.y + rad, rad, r.h - 2.0 * rad),
            color,
        );
        const SEG: usize = 6;
        use std::f32::consts::{FRAC_PI_2, PI};
        let corners = [
            (r.x + rad, r.y + rad, PI),
            (r.right() - rad, r.y + rad, 1.5 * PI),
            (r.right() - rad, r.bottom() - rad, 0.0),
            (r.x + rad, r.bottom() - rad, FRAC_PI_2),
        ];
        self.push_cmd(ATLAS, (4 * SEG * 3) as u32);
        let uv = [WHITE_UV[0], WHITE_UV[1]];
        let v = |x: f32, y: f32| Vertex {
            pos: [x, y],
            uv,
            color,
        };
        for (cx, cy, a0) in corners {
            for i in 0..SEG {
                let a = a0 + FRAC_PI_2 * i as f32 / SEG as f32;
                let b = a0 + FRAC_PI_2 * (i + 1) as f32 / SEG as f32;
                self.verts.extend_from_slice(&[
                    v(cx, cy),
                    v(cx + a.cos() * rad, cy + a.sin() * rad),
                    v(cx + b.cos() * rad, cy + b.sin() * rad),
                ]);
            }
        }
    }

    /// Rounded panel: `border` ring of width `t` around a `fill`.
    pub fn rounded_panel(&mut self, r: Rect, radius: f32, fill: Color, border: Color, t: f32) {
        self.rounded_rect(r, radius, border);
        self.rounded_rect(
            Rect::new(r.x + t, r.y + t, r.w - 2.0 * t, r.h - 2.0 * t),
            (radius - t).max(0.0),
            fill,
        );
    }

    /// Filled circle / ring approximated with a triangle fan.
    pub fn circle(
        &mut self,
        cx: f32,
        cy: f32,
        r_outer: f32,
        r_inner: f32,
        color: Color,
        segments: usize,
    ) {
        let n = segments.max(8);
        self.push_cmd(ATLAS, (n * 6) as u32);
        let uv = [WHITE_UV[0], WHITE_UV[1]];
        let p = |a: f32, r: f32| Vertex {
            pos: [cx + a.cos() * r, cy + a.sin() * r],
            uv,
            color,
        };
        for i in 0..n {
            let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
            let (o0, o1, i0, i1) = (
                p(a0, r_outer),
                p(a1, r_outer),
                p(a0, r_inner),
                p(a1, r_inner),
            );
            self.verts.extend_from_slice(&[o0, o1, i0, o1, i1, i0]);
        }
    }

    pub fn line_h(&mut self, x: f32, y: f32, w: f32, t: f32, color: Color) {
        self.rect(Rect::new(x, y - t * 0.5, w, t), color);
    }
}

pub struct GpuTexture {
    pub texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    pub w: u32,
    pub h: u32,
}

pub struct Gfx {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pipelines: [wgpu::RenderPipeline; 5],
    tex_layout: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    textures: Vec<Option<GpuTexture>>,
    vbuf: wgpu::Buffer,
    vcap: u64,
    pub present_modes: Vec<wgpu::PresentMode>,
    pub adapter_name: String,
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
        let instance = wgpu::util::new_instance_with_webgpu_detection(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        })
        .await;
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
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: choose_present_mode(&caps.present_modes, mode),
            desired_maximum_frame_latency: 1,
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
        let make = |blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("2d"),
            layout: Some(&layout),
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
                entry_point: Some("fs_main"),
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
        let pipelines = [
            make(wgpu::BlendState::ALPHA_BLENDING),
            make(color(F::SrcAlpha, F::One, O::Add)),
            make(color(F::SrcAlpha, F::One, O::ReverseSubtract)),
            make(color(F::Zero, F::Src, O::Add)),
            make(color(F::OneMinusDst, F::Zero, O::Add)),
        ];
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
            tex_layout,
            uniform_buf,
            uniform_bind,
            sampler,
            textures: Vec::new(),
            vbuf,
            vcap,
            present_modes: caps.present_modes.clone(),
            adapter_name: format!("{} ({:?})", info.name, info.backend),
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
        self.config.present_mode = choose_present_mode(&self.present_modes, mode);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn create_texture(&mut self, w: u32, h: u32, rgba: &[u8]) -> TexId {
        let size = wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let gt = {
            let view = texture.create_view(&Default::default());
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.tex_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            GpuTexture {
                texture,
                bind,
                w,
                h,
            }
        };
        let id = match self.textures.iter().position(|t| t.is_none()) {
            Some(i) => {
                self.textures[i] = Some(gt);
                i
            }
            None => {
                self.textures.push(Some(gt));
                self.textures.len() - 1
            }
        };
        self.update_texture(id, 0, 0, w, h, rgba);
        id
    }

    pub fn update_texture(&self, id: TexId, x: u32, y: u32, w: u32, h: u32, rgba: &[u8]) {
        let Some(Some(t)) = self.textures.get(id) else {
            return;
        };
        if w == 0 || h == 0 {
            return;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &t.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn texture_size(&self, id: TexId) -> Option<(u32, u32)> {
        self.textures
            .get(id)
            .and_then(|t| t.as_ref())
            .map(|t| (t.w, t.h))
    }

    pub fn free_texture(&mut self, id: TexId) {
        if id != ATLAS {
            if let Some(t) = self.textures.get_mut(id) {
                *t = None;
            }
        }
    }

    /// Load an image file. `color_key_black` makes pure black transparent
    /// (BMS BGA layer convention).
    pub fn load_image(&mut self, path: &std::path::Path, color_key_black: bool) -> Option<TexId> {
        let img = load_rgba(path, color_key_black)?;
        Some(self.create_texture(img.0, img.1, &img.2))
    }

    pub fn render(&mut self, batch: &Batch, clear: Color) -> Result<(), wgpu::SurfaceError> {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated) | Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(e) => return Err(e),
        };
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
                pass.set_pipeline(&self.pipelines[c.blend as usize]);
                pass.set_bind_group(1, &t.bind, &[]);
                pass.draw(c.start..c.start + c.count, 0..1);
            }
        }
        self.queue.submit(Some(enc.finish()));
        frame.present();
        Ok(())
    }
}

pub fn load_rgba(path: &std::path::Path, color_key_black: bool) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = match crate::platform::vfs::read(path) {
        Ok(b) => b,
        Err(e) => {
            log::debug!("image {}: {e}", path.display());
            return None;
        }
    };
    load_rgba_bytes(&bytes, color_key_black)
}

pub fn load_rgba_bytes(bytes: &[u8], color_key_black: bool) -> Option<(u32, u32, Vec<u8>)> {
    // TGA has no signature to recognise it by
    let img = match image::load_from_memory(bytes).or_else(|e| {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Tga).map_err(|_| e)
    }) {
        Ok(i) => i,
        Err(e) => {
            log::debug!("image decode: {e}");
            return None;
        }
    };
    let mut rgba = img.to_rgba8();
    if color_key_black {
        for p in rgba.pixels_mut() {
            if p[0] == 0 && p[1] == 0 && p[2] == 0 {
                p[3] = 0;
            }
        }
    }
    let (w, h) = rgba.dimensions();
    Some((w, h, rgba.into_raw()))
}

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
        P::LowLatency => pick(&[W::Mailbox, W::Immediate, W::FifoRelaxed, W::Fifo]),
        P::VSync => W::Fifo,
        P::Immediate => pick(&[W::Immediate, W::Mailbox, W::Fifo]),
    }
}
