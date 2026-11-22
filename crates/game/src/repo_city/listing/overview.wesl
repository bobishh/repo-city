import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
};

struct ReadingBands {
    states: vec4<u32>,
    geometry: vec4<f32>,
    partial_ink: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> overview: ReadingBands;

@group(constants::MATERIAL_BIND_GROUP) @binding(101)
var open_print: texture_2d<f32>;

@group(constants::MATERIAL_BIND_GROUP) @binding(102)
var open_sampler: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    let pbr = pbr_input_from_standard_material(in, front);
    let uv = vec2<f32>(in.uv.x, in.uv.y * overview.geometry.x);
    let clear = textureSample(open_print, open_sampler, uv);
    let band = min(u32(in.uv.y * overview.geometry.y), u32(overview.geometry.y) - 1u);
    let state = (overview.states[band / 16u] >> ((band % 16u) * 2u)) & 3u;
    let ink = select(pbr.material.base_color, clear, state == 2u);
    let edge = max(fwidth(in.uv.x) * 0.5, 0.0001);
    let gutter = smoothstep(overview.geometry.z - edge, overview.geometry.z + edge, in.uv.x)
        - smoothstep(overview.geometry.w - edge, overview.geometry.w + edge, in.uv.x);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(
        pbr, mix(ink, overview.partial_ink, select(0.0, gutter, state == 1u)));
    return out;
}
