import { test, expect } from "@playwright/test";

test("a guest can recover a survey buoy and read a station by holding the light on it", async ({ page }, info) => {
  const sha = "b".repeat(40);
  const source = Array.from({ length: 42 }, (_, i) =>
    `pub const SENSOR_${i}: &str = "temperature depth salinity oxygen turbidity pressure conductivity fluorescence";`).join("\n");
  const faults = [];
  page.on("pageerror", error => faults.push(error.message));
  page.on("console", entry => {
    if (/Quitting the application|Caught rendering error|Shader .*error|panicked at/i.test(entry.text())) faults.push(entry.text());
  });
  await page.addInitScript(() => {
    localStorage.setItem("repo-city-reading-v1", JSON.stringify({
      mode: "delayed", block_kind: "lines", block_lines: 3, auto_advance: false,
      min_seconds: 1, max_seconds: 1,
    }));
    let locked = null;
    Object.defineProperty(document, "pointerLockElement", { get: () => locked });
    HTMLCanvasElement.prototype.requestPointerLock = function () {
      locked = this;
      queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange")));
      return Promise.resolve();
    };
    document.exitPointerLock = () => {
      locked = null;
      queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange")));
    };
  });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    const file = { path: "stations/sensor.rs", type: "blob", mode: "100644", sha, size: source.length };
    return route.fulfill({ json: path === "/repos/owner/trench" ? { default_branch: "main" }
      : path.includes("/commits/") ? { sha, parents: [] }
      : path.includes("/git/trees/") ? { tree: [file] }
      : { encoding: "base64", content: Buffer.from(source).toString("base64"), size: source.length } });
  });
  const canvas = page.locator("#repo-city");
  const summary = page.locator("#progress-summary");
  const buoys = () => page.evaluate(() => {
    const key = Object.keys(localStorage).find(key => key.endsWith("|world=abyss|survey-buoys"));
    return key ? JSON.parse(localStorage.getItem(key)) : [];
  });
  const board = async () => {
    await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
    await expect(page.locator("#city-file")).toHaveText("sensor.rs", { timeout: 60_000 });
    await canvas.focus();
    await canvas.evaluate(element => element.requestPointerLock());
    await expect(canvas).toHaveAttribute("data-pointer-locked", "");
  };
  await page.goto("/?world=abyss&repo=owner%2Ftrench");
  await board();
  await expect(summary).toContainText("0 lines revealed");
  await page.keyboard.down("KeyI");
  try {
    await expect(canvas).toHaveAttribute("data-abyss-exterior", "");
    await canvas.screenshot({ path: info.outputPath("engineer.png") });
    await expect(summary).toContainText("0 lines revealed");
  } finally {
    await page.keyboard.up("KeyI");
  }
  await expect(canvas).not.toHaveAttribute("data-abyss-exterior", "");
  await page.keyboard.press("KeyO", { delay: 180 });
  await expect.poll(buoys).toHaveLength(1);
  await canvas.screenshot({ path: info.outputPath("survey-buoy.png") });
  await page.reload();
  await board();
  await expect.poll(buoys).toHaveLength(1);
  await page.keyboard.press("KeyO", { delay: 180 });
  await expect.poll(buoys).toHaveLength(0);
  await expect(summary).toContainText("0 lines revealed");
  await page.keyboard.press("KeyE", { delay: 180 });
  await expect(summary).toContainText(/[1-9]\d* lines revealed/, { timeout: 45_000 });
  const png = await canvas.screenshot({ path: info.outputPath("lit-source.png") });
  const ink = await page.evaluate(async encoded => {
    const bytes = Uint8Array.from(atob(encoded), char => char.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const surface = new OffscreenCanvas(bitmap.width, bitmap.height);
    const ctx = surface.getContext("2d");
    ctx.drawImage(bitmap, 0, 0);
    bitmap.close();
    // Compare glyph highlights in the upper corners with the middle of the same source wall.
    const samples = [[0.17, 0.17], [0.50, 0.45], [0.77, 0.17], [0.45, 0.63]];
    return samples.map(([x, y]) => {
      const { data } = ctx.getImageData(x * surface.width, y * surface.height, 70, 64);
      const values = [];
      for (let i = 0; i < data.length; i += 4) values.push(Math.max(data[i], data[i + 1], data[i + 2]));
      values.sort((a, b) => a - b);
      return values[Math.floor(values.length * 0.98)];
    });
  }, png.toString("base64"));
  const backing = ink.pop();
  expect(Math.min(...ink), `glyph brightness across reading surface: ${ink}`).toBeGreaterThan(100);
  expect(Math.min(...ink) / Math.max(...ink)).toBeGreaterThan(0.65);
  expect(backing, "empty station plating below the last line stays dark").toBeLessThan(100);
  expect(faults).toEqual([]);
});
