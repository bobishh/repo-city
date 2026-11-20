import assert from "node:assert/strict";
import test from "node:test";
import { createCityShaderModule } from "./webgpu-shader.mjs";

const texture = "_1bevy_pbr_render__1pbr_bindings__2base_color_texture";
const sampler = "_1bevy_pbr_render__1pbr_bindings__2base_color_sampler";
const code = `fn sample(uv: vec2f) -> vec4f {
    let texture = ${texture};
    let sampler_ = ${sampler};
    let coords = uv;
    return textureSampleBias(
        texture,
        sampler_,
        coords, 0.);
}`;
const device = { createShaderModule: descriptor => descriptor };

test("uses Bevy's bindings directly on browsers without texture and sampler lets", () => {
  const original = { label: "bevy pbr", code };
  const result = createCityShaderModule(device, original, new Set());
  assert.equal(result.label, original.label);
  assert.doesNotMatch(result.code, /let (texture|sampler_)/);
  assert.match(result.code, /let coords = uv;/);
  assert.ok(result.code.includes(`${texture}, ${sampler},`));
  assert.equal(original.code, code);
});

test("passes through supported shaders and shaders outside the Bevy PBR pattern", () => {
  const original = { code };
  const other = { code: code.replaceAll("_1bevy_pbr", "other") };
  assert.equal(createCityShaderModule(device, original, new Set(["texture_and_sampler_let"])), original);
  assert.equal(createCityShaderModule(device, other, new Set()), other);
});
