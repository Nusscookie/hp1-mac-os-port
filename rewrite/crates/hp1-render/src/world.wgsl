// World surfaces: texture × light map × 2 (the game's light maps are stored at half strength).

struct Globals {
    view_proj: mat4x4<f32>,
    // x: brightness
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var tex_sampler: sampler;
@group(2) @binding(0) var lightmap: texture_2d<f32>;
@group(2) @binding(1) var lightmap_sampler: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) lm_uv: vec2<f32>,
};

@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) lm_uv: vec2<f32>) -> VOut {
    var o: VOut;
    o.pos = g.view_proj * vec4<f32>(pos, 1.0);
    o.uv = uv;
    o.lm_uv = lm_uv;
    return o;
}

fn shade(i: VOut) -> vec4<f32> {
    let c = textureSample(tex, tex_sampler, i.uv);
    let light = textureSample(lightmap, lightmap_sampler, max(i.lm_uv, vec2<f32>(0.0))).rgb * 2.0;
    let lit = select(light, vec3<f32>(1.0), i.lm_uv.x < 0.0);
    return vec4<f32>(min(c.rgb * lit * g.params.x, vec3<f32>(1.0)), c.a);
}

@fragment
fn fs_opaque(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(shade(i).rgb, 1.0);
}

@fragment
fn fs_masked(i: VOut) -> @location(0) vec4<f32> {
    let c = shade(i);
    if c.a < 0.5 {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}

// Translucent (added on top) and modulated (multiplied) surfaces use the colour only; the blend
// state does the rest.
@fragment
fn fs_color(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(shade(i).rgb, 1.0);
}

@fragment
fn fs_modulated(i: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, tex_sampler, i.uv);
    return vec4<f32>(c.rgb, 1.0);
}
