import { defineConfig } from "@playwright/test";
import { join } from "node:path";
import { tmpdir } from "node:os";

const port = process.env.REPO_CITY_TEST_PORT ?? "18791";
const hardware = process.env.REPO_CITY_TEST_GPU === "hardware";
const graphicsArgs = hardware
  ? ["--enable-unsafe-webgpu", "--window-position=-10000,-10000"]
  : ["--enable-unsafe-webgpu", "--enable-unsafe-swiftshader", "--use-angle=swiftshader"];
if (!hardware && process.platform === "linux") {
  // The WebGPU canvas needs a Vulkan compositor to share images with SwiftShader.
  graphicsArgs.push("--use-vulkan=swiftshader", "--enable-features=Vulkan");
}
export default defineConfig({
  testDir: "./tests",
  workers: 1,
  timeout: 180_000,
  outputDir: process.env.REPO_CITY_TEST_OUTPUT ?? join(tmpdir(), "repo-city-playwright"),
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    headless: !hardware,
    launchOptions: { args: graphicsArgs },
    screenshot: "only-on-failure",
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: `node tests/serve.mjs ${port}`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
  },
});
