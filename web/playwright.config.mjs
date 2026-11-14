import { defineConfig } from "@playwright/test";
import { join } from "node:path";
import { tmpdir } from "node:os";

const port = process.env.REPO_CITY_TEST_PORT ?? "18791";
export default defineConfig({
  testDir: "./tests",
  workers: 1,
  timeout: 120_000,
  outputDir: process.env.REPO_CITY_TEST_OUTPUT ?? join(tmpdir(), "repo-city-playwright"),
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    launchOptions: { args: ["--enable-unsafe-webgpu", "--enable-unsafe-swiftshader", "--use-angle=swiftshader"] },
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: `node tests/serve.mjs ${port}`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
  },
});
