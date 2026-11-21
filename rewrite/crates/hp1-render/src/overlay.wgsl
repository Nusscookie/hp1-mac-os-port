// Screen-space tiles for the HUD: texture × colour, positions in HUD units.

struct Globals {
    // x, y: screen size in HUD units
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var tex_sampler: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(pos.x / g.size.x * 2.0 - 1.0, 1.0 - pos.y / g.size.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    return o;
}

@fragment
fn fs_normal(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>((textureSample(tex, tex_sampler, i.uv) * i.color).rgb, 1.0);
}

@fragment
fn fs_masked(i: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, tex_sampler, i.uv) * i.color;
    if c.a < 0.5 {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}
