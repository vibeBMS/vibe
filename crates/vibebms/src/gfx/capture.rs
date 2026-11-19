//! Screenshots from the surface: a frame's image copied into a buffer and
//! mapped without waiting, read some frames later.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

const MAPPING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// A frame copied into `buf`, `row` bytes a row, being mapped.
pub(super) struct Pending {
    buf: wgpu::Buffer,
    width: u32,
    height: u32,
    row: u32,
    /// The surface keeps blue first (BGRA).
    bgra: bool,
    state: Arc<AtomicU8>,
}

/// The surface's texture formats a screenshot can be read from: 8-bit
/// RGBA or BGRA (blue first: `Some(true)`).
fn bgra(f: wgpu::TextureFormat) -> Option<bool> {
    use wgpu::TextureFormat as F;
    match f {
        F::Bgra8Unorm | F::Bgra8UnormSrgb => Some(true),
        F::Rgba8Unorm | F::Rgba8UnormSrgb => Some(false),
        _ => None,
    }
}

/// A screenshot can be read from a surface of format `f`.
pub(super) fn readable(f: wgpu::TextureFormat) -> bool {
    bgra(f).is_some()
}

/// Bytes of a row, as a texture-to-buffer copy wants them aligned.
fn padded_row(width: u32) -> u32 {
    let a = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    (width * 4).div_ceil(a) * a
}

/// Copy `tex` into a new buffer with `enc`; [`Pending::start`] once the
/// copy is submitted. `None`: its format cannot be read.
pub(super) fn copy(
    device: &wgpu::Device,
    enc: &mut wgpu::CommandEncoder,
    tex: &wgpu::Texture,
) -> Option<Pending> {
    let bgra = bgra(tex.format())?;
    let (width, height) = (tex.width(), tex.height());
    let row = padded_row(width);
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot"),
        size: row as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        tex.size(),
    );
    Some(Pending {
        buf,
        width,
        height,
        row,
        bgra,
        state: Arc::new(AtomicU8::new(MAPPING)),
    })
}

impl Pending {
    /// Ask for the buffer to be mapped (once its copy is submitted).
    pub(super) fn start(&self) {
        let state = self.state.clone();
        self.buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            state.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
        });
    }

    /// The image once mapped (RGBA rows, opaque), else `None` while it is
    /// being mapped.
    pub(super) fn take(&self) -> Option<Result<image::RgbaImage, String>> {
        match self.state.load(Ordering::Acquire) {
            MAPPING => None,
            FAILED => Some(Err("the frame could not be read".into())),
            _ => {
                let Ok(data) = self.buf.slice(..).get_mapped_range() else {
                    return Some(Err("the frame could not be read".into()));
                };
                let mut px = Vec::with_capacity((self.width * self.height * 4) as usize);
                for line in data.chunks(self.row as usize) {
                    for p in line[..(self.width * 4) as usize].as_chunks::<4>().0 {
                        let (r, b) = if self.bgra {
                            (p[2], p[0])
                        } else {
                            (p[0], p[2])
                        };
                        px.extend_from_slice(&[r, p[1], b, 255]);
                    }
                }
                drop(data);
                self.buf.unmap();
                Some(
                    image::RgbaImage::from_raw(self.width, self.height, px)
                        .ok_or_else(|| "the frame's size".to_string()),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rows_are_aligned() {
        assert_eq!(super::padded_row(64), 256);
        assert_eq!(super::padded_row(65), 512);
        assert_eq!(super::padded_row(1280), 5120);
    }
}
