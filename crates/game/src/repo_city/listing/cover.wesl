import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
};
import bevy_render::color_operations::srgb_to_linear;

struct CoverRows {
    info: vec4<u32>,
    glass: vec4<f32>,
    source_changes: array<vec4<f32>, 3>,
    closed_accent: vec4<f32>,
    closed_stroke: vec4<f32>,
    spans: array<vec4<f32>, 16>,
    paint: array<vec4<f32>, 16>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> cover: CoverRows;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    let pbr = pbr_input_from_standard_material(in, front);
    var ink = pbr.material.base_color;
    if (cover.info.y > 0u) {
        let row = min(u32(in.uv.y * f32(cover.info.y)), cover.info.y - 1u);
        let paint = cover.paint[row];
        if ((cover.info.x >> row) & 1u) == 0u {
            let x = in.uv.x * f32(cover.info.z);
            let y = fract(in.uv.y * f32(cover.info.y)) * f32(cover.info.w);
            let span = cover.spans[row];
            var rgb = cover.glass.rgb;
            // Keep rasterized line numbers, indexed on the left of the source gutter.
            if x < 90.0 && max(ink.r, max(ink.g, ink.b)) > 0.035 {
                rgb = mix(rgb, vec3<f32>(127.0, 168.0, 181.0), clamp(ink.b * 2.3, 0.0, 1.0));
            }
            if x >= 91.0 && x < 96.0 {
                rgb = cover.closed_accent.rgb;
            }
            if x >= 101.0 && x < 105.0 {
                rgb = cover.source_changes[u32(span.z)].rgb;
            }
            if x >= span.x && x < span.y && y >= 12.0 && y < 15.0 {
                rgb = cover.closed_stroke.rgb;
            }
            ink = vec4<f32>(srgb_to_linear(rgb / 255.0), ink.a);
        }
        if paint.a > 0.0 {
            let x = in.uv.x;
            let y = fract(in.uv.y * f32(cover.info.y));
            let grain = fract(dot(floor(vec2<f32>(x * 640.0, y * 21.0)), vec2<f32>(0.75488, 0.56984)));
            let edge = 0.91 + (grain - 0.5) * 0.032;
            let shape = smoothstep(0.025, 0.055, x) * (1.0 - smoothstep(edge - 0.06, edge, x));
            let band = smoothstep(0.0, 0.16, y) * (1.0 - smoothstep(0.84, 1.0, y));
            let gutter = smoothstep(0.065, 0.071, x) * (1.0 - smoothstep(0.082, 0.09, x));
            let opacity = paint.a * max(shape * band * (0.22 + 0.20 * grain), gutter * band * 0.96);
            ink = vec4<f32>(mix(ink.rgb, srgb_to_linear(paint.rgb), opacity), ink.a);
        }
    }
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr, ink);
    return out;
}
