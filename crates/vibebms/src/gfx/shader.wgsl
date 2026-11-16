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

// Distance-field glyphs as beatoraja draws them (its distance_field.frag):
// the edge at 0.5, an outline out to `outline_distance`, and a shadow
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

@fragment
fn fs_sdf(i: VsOut) -> @location(0) vec4<f32> {
    let dist = textureSample(tex, samp, i.uv).a;
    let fill = smoothstep(0.5 - SMOOTHING, 0.5 + SMOOTHING, dist);
    let c = mix(sdf.outline_color, i.color, fill);
    let a = smoothstep(sdf.outline_distance - SMOOTHING, sdf.outline_distance + SMOOTHING, dist);
    let main = vec4<f32>(c.rgb, c.a * a);
    let off = sdf.shadow_offset / vec2<f32>(textureDimensions(tex));
    let sd = textureSample(tex, samp, i.uv - off).a;
    // no smoothing: a hard edge (smoothstep needs low < high)
    let s = max(sdf.shadow_smoothing, 0.0001);
    let sa = smoothstep(0.5 - s, 0.5 + s, sd);
    let shadow = vec4<f32>(sdf.shadow_color.rgb, sdf.shadow_color.a * sa);
    return mix(shadow, main, main.a);
}
