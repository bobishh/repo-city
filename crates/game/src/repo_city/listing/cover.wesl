import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
};
import bevy_render::color_operations::srgb_to_linear;
import bevy_pbr::render::pbr_bindings::{base_color_texture, base_color_sampler};

// Nine fixed samples keep unread glyphs recognizable as structure, without a full-screen pass.
fn frost(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(base_color_texture));
    let center = (floor(uv * size / 4.0) * 4.0 + 2.0) / size;
    var color = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * 4.0 / size;
            color += textureSampleLevel(base_color_texture, base_color_sampler, clamp(center + offset, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
        }
    }
    return color / 9.0;
}

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
            let glass = srgb_to_linear(cover.glass.rgb / 255.0);
            let blurred = frost(in.uv);
            let grain = fract(dot(floor(in.uv * vec2<f32>(640.0, 512.0)), vec2<f32>(0.75488, 0.56984)));
            let icy = mix(glass, vec3<f32>(0.15, 0.25, 0.31), 0.12 + grain * 0.05);
            // Preserve clear line numbers and change markers; frost only the code.
            if x >= 106.0 {
                ink = vec4<f32>(mix(blurred * vec3<f32>(0.75, 0.95, 1.12), icy, 0.24), ink.a);
            }
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
