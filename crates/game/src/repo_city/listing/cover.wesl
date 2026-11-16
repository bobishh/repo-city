import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
};
import bevy_render::color_operations::srgb_to_linear;

struct CoverRows {
    info: vec4<u32>,
    spans: array<vec4<f32>, 16>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> cover: CoverRows;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    let pbr = pbr_input_from_standard_material(in, front);
    var ink = pbr.material.base_color;
    if (cover.info.y > 0u) {
        let row = min(u32(in.uv.y * f32(cover.info.y)), cover.info.y - 1u);
        if ((cover.info.x >> row) & 1u) == 0u {
            let x = in.uv.x * f32(cover.info.z);
            let y = fract(in.uv.y * f32(cover.info.y)) * f32(cover.info.w);
            let span = cover.spans[row];
            var rgb = vec3<f32>(28.0, 43.0, 50.0);
            if span.z == 1.0 { rgb = vec3<f32>(27.0, 56.0, 48.0); }
            if span.z == 2.0 { rgb = vec3<f32>(64.0, 40.0, 47.0); }
            // Keep rasterized line numbers, indexed on the left of the source gutter.
            if x < 90.0 && max(ink.r, max(ink.g, ink.b)) > 0.035 {
                rgb = mix(rgb, vec3<f32>(127.0, 168.0, 181.0), clamp(ink.b * 2.3, 0.0, 1.0));
            }
            if x >= 91.0 && x < 96.0 {
                rgb = select(vec3<f32>(35.0, 69.0, 77.0), vec3<f32>(83.0, 148.0, 155.0), (u32(x + y) % 12u) < 5u);
            }
            if x >= span.x && x < span.y && y >= 12.0 && y < 15.0 {
                rgb = vec3<f32>(68.0, 91.0, 101.0);
            }
            ink = vec4<f32>(srgb_to_linear(rgb / 255.0), ink.a);
        }
    }
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr, ink);
    return out;
}
