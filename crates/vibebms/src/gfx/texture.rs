//! Textures on the GPU: made, updated, freed; images loaded into them.

use super::{Gfx, TexId, ATLAS};

pub struct GpuTexture {
    pub texture: wgpu::Texture,
    pub(super) bind: wgpu::BindGroup,
    pub w: u32,
    pub h: u32,
}

impl Gfx {
    /// The size a `w`×`h` image is stored at: scaled down to the GPU's
    /// largest texture (coordinates keep using the image's own size).
    fn stored_size(&self, w: u32, h: u32) -> (u32, u32) {
        let max = self.device.limits().max_texture_dimension_2d;
        if w <= max && h <= max {
            return (w.max(1), h.max(1));
        }
        let s = max as f64 / w.max(h) as f64;
        let f = |v: u32| ((v as f64 * s) as u32).clamp(1, max);
        (f(w), f(h))
    }

    /// A `w`×`h` texture holding `rgba` (transparent when `rgba` is shorter).
    pub fn create_texture(&mut self, w: u32, h: u32, rgba: &[u8]) -> TexId {
        let (sw, sh) = self.stored_size(w, h);
        if (sw, sh) != (w, h) && w > 0 && h > 0 {
            log::warn!("{w}x{h} image is larger than the GPU allows; scaled to {sw}x{sh}");
        }
        let size = wgpu::Extent3d {
            width: sw,
            height: sh,
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
        if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
            return;
        }
        let stored = t.texture.size();
        let scaled;
        let (w, h, rgba) = if (stored.width, stored.height) == (t.w, t.h) {
            (w, h, rgba)
        } else if (x, y, w, h) == (0, 0, t.w, t.h) {
            // a whole image larger than the GPU allows
            let px = &rgba[..(w * h * 4) as usize];
            let Some(img) = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, px) else {
                return;
            };
            let (sw, sh) = (stored.width, stored.height);
            scaled = image::imageops::resize(&img, sw, sh, image::imageops::FilterType::Triangle);
            (sw, sh, scaled.as_raw().as_slice())
        } else {
            return;
        };
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
}
