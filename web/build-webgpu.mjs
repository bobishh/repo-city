import { copyFile, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";

const gluePath = process.argv[2];
const source = await readFile(gluePath, "utf8");
const [before, after, ...rest] = source.split("const ret = arg0.createShaderModule(arg1);");
if (after === undefined || rest.length) {
  throw new Error("Expected one createShaderModule call in generated wasm-bindgen glue");
}

await copyFile(new URL("./webgpu-shader.mjs", import.meta.url), join(dirname(gluePath), "webgpu-shader.mjs"));
await writeFile(gluePath,
  `import { createCityShaderModule } from "./webgpu-shader.mjs";\n${before}const ret = createCityShaderModule(arg0, arg1);${after}`);
