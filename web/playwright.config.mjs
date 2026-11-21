import { defineConfig } from "@playwright/test";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const port = process.env.REPO_CITY_TEST_PORT ?? "18791";
const partyFixture = process.env.REPO_CITY_TEST_PARTY === "1";
const hardware = process.env.REPO_CITY_TEST_GPU === "hardware";
const visible = process.env.REPO_CITY_TEST_VISIBLE === "1";
const browserName = process.env.REPO_CITY_TEST_BROWSER ?? "chromium";
const graphicsArgs = hardware
  ? ["--enable-unsafe-webgpu", ...(process.platform === "darwin" ? ["--use-angle=metal"] : [])]
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
    browserName,
    headless: !visible,
    launchOptions: browserName === "chromium" ? { args: graphicsArgs } : { firefoxUserPrefs: { "dom.webgpu.enabled": true } },
    screenshot: "only-on-failure",
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: partyFixture ? "mix run --no-halt scripts/party_fixture.exs" : "mix run --no-halt",
    cwd: fileURLToPath(new URL("../server/", import.meta.url)),
    env: {
      MIX_ENV: partyFixture ? "test" : "dev",
      PHX_SERVER: "true",
      BIND_ADDRESS: "127.0.0.1",
      PORT: port,
      PUBLIC_ORIGIN: `http://127.0.0.1:${port}`,
    },
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
  },
});
