// Bevy 0.20 emits these PBR aliases even without texture_and_sampler_let.
// Substitute only the texture/sampler pair at its sample call on those GPUs.
const pbrAliases = /(^[ \t]*)let texture = (_\d+bevy_pbr_render__\d+pbr_bindings_\w+_texture);\r?\n\1let sampler_ = (_\d+bevy_pbr_render__\d+pbr_bindings_\w+_sampler);\r?\n([^{}]*?\btextureSample(?:Bias|Grad)?\(\s*)texture,\s*sampler_,/gm;

export function createCityShaderModule(device, descriptor, features = globalThis.navigator?.gpu?.wgslLanguageFeatures) {
  if (features?.has("texture_and_sampler_let")) return device.createShaderModule(descriptor);

  const code = descriptor.code.replace(pbrAliases,
    (_match, _indent, texture, sampler, sample) => `${sample}${texture}, ${sampler},`);
  return device.createShaderModule(code === descriptor.code ? descriptor : { ...descriptor, code });
}
