struct Uniforms {
    screen: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(pos.x / u.screen.x * 2.0 - 1.0, 1.0 - pos.y / u.screen.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    return o;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, i.uv) * i.color;
}

// Distance-field glyphs (the page's alpha is the distance, 0.5 at the
// edge, larger inside): an outline out to `outline_distance`, and a shadow
// sampled `shadow_offset` texels away, softened by `shadow_smoothing`.
struct Sdf {
    outline_color: vec4<f32>,
    shadow_color: vec4<f32>,
    shadow_offset: vec2<f32>,
    outline_distance: f32,
    shadow_smoothing: f32,
};

@group(2) @binding(0) var<uniform> sdf: Sdf;

const SMOOTHING: f32 = 0.0625;

// fs_sdf was written from a behaviour specification of the text style,
// not from another program's code.
@fragment
fn fs_sdf(i: VsOut) -> @location(0) vec4<f32> {
    let d = textureSample(tex, samp, i.uv).a;
    // the outline colour outside the glyph's edge, the text colour inside
    let fill = smoothstep(0.5 - SMOOTHING, 0.5 + SMOOTHING, d);
    let body = mix(sdf.outline_color, i.color, fill);
    // the shape reaches out to the outline's end
    let e = sdf.outline_distance;
    let cover = smoothstep(e - SMOOTHING, e + SMOOTHING, d);
    let front = vec4<f32>(body.rgb, body.a * cover);
    // the glyph's shape (no outline) moved by the offset, in page texels
    let page = vec2<f32>(textureDimensions(tex));
    let ds = textureSample(tex, samp, i.uv - sdf.shadow_offset / page).a;
    let soft = max(sdf.shadow_smoothing, 0.0001);
    let shade = smoothstep(0.5 - soft, 0.5 + soft, ds);
    let shadow = vec4<f32>(sdf.shadow_color.rgb, sdf.shadow_color.a * shade);
    // from the shadow to the front layer by the front's alpha, all four
    // channels alike
    return mix(shadow, front, front.a);
}
