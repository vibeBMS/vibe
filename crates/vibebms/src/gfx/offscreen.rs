//! Drawing without a window (desktop): each frame drawn into a texture
//! and read back (`vibebms export video`).

use super::{Batch, Color, Gfx};

/// A [`Gfx`] that draws into a texture of a fixed size.
pub struct Offscreen {
    pub gfx: Gfx,
    target: wgpu::Texture,
}

/// The texture frames are drawn into: 8-bit RGBA (read back as it is).
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Offscreen {
    /// A GPU device with no surface (a software one, such as Mesa's
    /// lavapipe, where there is no GPU), drawing frames of `w` x `h`.
    pub fn new(w: u32, h: u32) -> Result<Offscreen, String> {
        pollster::block_on(Self::create(w.max(1), h.max(1)))
    }

    async fn create(w: u32, h: u32) -> Result<Offscreen, String> {
        let instance =
            wgpu::Instance::new(super::device::instance_desc(wgpu::Backends::all(), None));
        let mut adapter = None;
        for fallback in [false, true] {
            adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: fallback,
                    apply_limit_buckets: false,
                })
                .await
                .ok();
            if adapter.is_some() {
                break;
            }
        }
        let adapter =
            adapter.ok_or("no GPU adapter (nor a software one) can draw without a window")?;
        let info = adapter.get_info();
        log::info!("gpu (headless): {} ({:?})", info.name, info.backend);
        let max = adapter.limits().max_texture_dimension_2d;
        if w > max || h > max {
            return Err(format!(
                "{w}x{h} is larger than this GPU draws ({max}x{max})"
            ));
        }
        let (device, queue) = adapter
            .request_device(&super::device::device_desc("vibebms headless", &adapter))
            .await
            .map_err(|e| e.to_string())?;
        device.on_uncaptured_error(std::sync::Arc::new(|e| log::error!("gpu: {e}")));
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        let config = wgpu::SurfaceConfiguration {
            usage,
            format: FORMAT,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 1,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        let caps = wgpu::SurfaceCapabilities {
            formats: vec![FORMAT],
            // (not read)
            format_capabilities: Vec::new(),
            present_modes: vec![wgpu::PresentMode::Fifo],
            alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
            usages: usage,
        };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage,
            view_formats: &[],
        });
        let mut gfx = Gfx::with_device(device, queue, config, &caps, &info, None);
        // (the size's uniforms)
        gfx.resize(w, h);
        Ok(Offscreen { gfx, target })
    }

    /// Draw `batch` and read the frame back: RGBA rows, opaque.
    pub fn frame(&mut self, batch: &Batch, clear: Color) -> Result<Vec<u8>, String> {
        let gfx = &mut self.gfx;
        let mut enc = gfx.encode(batch, clear, &self.target);
        let copy = super::capture::copy(&gfx.device, &mut enc, &self.target)
            .ok_or("the frame's format cannot be read")?;
        gfx.queue.submit(Some(enc.finish()));
        copy.start();
        let _ = gfx.device.poll(wgpu::PollType::wait_indefinitely());
        loop {
            if let Some(img) = copy.take() {
                return img.map(|i| i.into_raw());
            }
            let _ = gfx.device.poll(wgpu::PollType::wait_indefinitely());
            std::thread::yield_now();
        }
    }
}
