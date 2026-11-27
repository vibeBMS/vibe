//! A frame's geometry: quads and shapes by texture, clip, blend mode and
//! glyph style, merged into as few draws as they allow.

use super::{Color, Rect, TexId, ATLAS, WHITE_UV};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
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

/// How distance-field glyphs are drawn (see `fs_sdf` in the shader):
/// beatoraja's outline and shadow for `.fnt` fonts of type 1 / 2.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct SdfStyle {
    pub outline_color: Color,
    pub shadow_color: Color,
    /// Texels of the glyph page (right, down).
    pub shadow_offset: [f32; 2],
    /// Distance where the outline ends (0.5 = no outline).
    pub outline_distance: f32,
    pub shadow_smoothing: f32,
}

pub(super) struct Cmd {
    pub(super) tex: TexId,
    pub(super) start: u32,
    pub(super) count: u32,
    pub(super) clip: Option<[u32; 4]>,
    pub(super) blend: Blend,
    /// Drawn as distance-field glyphs with this style (index in `styles`).
    pub(super) sdf: Option<u32>,
}

/// Geometry for one frame.
#[derive(Default)]
pub struct Batch {
    pub(super) verts: Vec<Vertex>,
    pub(super) cmds: Vec<Cmd>,
    clip: Option<[u32; 4]>,
    /// Outer clip every `set_clip` stays inside (a skin's viewport).
    base_clip: Option<Rect>,
    blend: Blend,
    pub(super) styles: Vec<SdfStyle>,
    sdf: Option<u32>,
}

#[cfg(test)]
impl Batch {
    /// The draws recorded: each one's texture and its triangles' vertices.
    pub fn draws(&self) -> impl Iterator<Item = (TexId, &[Vertex])> {
        let verts = |c: &Cmd| &self.verts[c.start as usize..(c.start + c.count) as usize];
        self.cmds.iter().map(move |c| (c.tex, verts(c)))
    }
}

impl Batch {
    pub fn clear(&mut self) {
        self.verts.clear();
        self.cmds.clear();
        self.clip = None;
        self.base_clip = None;
        self.blend = Blend::Alpha;
        self.styles.clear();
        self.sdf = None;
    }

    /// Draw following quads as distance-field glyphs (`None`: as images).
    pub fn set_sdf(&mut self, style: Option<SdfStyle>) {
        self.sdf = style.map(|st| {
            if self.styles.last() != Some(&st) {
                self.styles.push(st);
            }
            self.styles.len() as u32 - 1
        });
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
                    && c.sdf == self.sdf
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
                sdf: self.sdf,
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

    /// A four-cornered shape (top-left, top-right, bottom-right,
    /// bottom-left) with a colour at each corner.
    pub fn quad4(&mut self, p: [[f32; 2]; 4], colors: [Color; 4]) {
        if colors.iter().all(|c| c[3] <= 0.0) {
            return;
        }
        self.push_cmd(ATLAS, 6);
        let uv = [WHITE_UV[0], WHITE_UV[1]];
        let v = |i: usize| Vertex {
            pos: p[i],
            uv,
            color: colors[i],
        };
        self.verts
            .extend_from_slice(&[v(0), v(1), v(3), v(1), v(2), v(3)]);
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
}
