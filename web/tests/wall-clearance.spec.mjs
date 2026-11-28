import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    let locked = null;
    Object.defineProperty(document, "pointerLockElement", { get: () => locked });
    HTMLCanvasElement.prototype.requestPointerLock = function () {
      locked = this;
      queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange")));
      return Promise.resolve();
    };
    localStorage.setItem("repo-city-guide-v1", "seen");
  });
});

test("G and B reach readable walls on a tall viewport between neighboring buildings", async ({ page }, info) => {
  await page.setViewportSize({ width: 1440, height: 1200 });
  const head = "b".repeat(40), base = "a".repeat(40);
  const path = "src/7.rs";
  const source = `fn read() -> u32 { 1 }\n${"// Enough text for a neighboring facade to reach the reading height.\n".repeat(160)}`;
  const beforeSource = source.replace("{ 1 }", "{ 0 }");
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route(/\/github\/blame\?/, route => route.fulfill({ status: 401, json: {} }));
  await page.route(/\/github\/public\/|https:\/\/api.github.com\//, route => {
    const api = new URL(route.request().url()).pathname;
    const before = api.endsWith(base);
    const blob = before ? beforeSource : source;
    return route.fulfill({ json: api.endsWith("/repos/owner/closely-packed") ? { default_branch: "main" }
      : api.includes("/commits/") ? { sha: head, parents: [{ sha: base }] }
      : api.includes("/git/trees/") ? { tree: Array.from({ length: 9 }, (_, index) => ({
        path: `src/${index}.rs`, type: "blob", mode: "100644", sha: index === 7 && before ? base : head,
        size: source.length,
      })) }
      : { size: blob.length, encoding: "base64", content: Buffer.from(blob).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Fclosely-packed");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect(page.locator("#city-file")).toHaveText("7.rs");
  const canvas = page.locator("canvas");
  await canvas.evaluate(element => element.requestPointerLock());
  const sideOffset = direction => page.evaluate(({ path, direction }) => {
    for (const key of Object.keys(localStorage)) {
      if (key.startsWith("repo-city-last-view-v1:")) {
        const view = JSON.parse(localStorage.getItem(key));
        const x = direction * view.offset[0];
        if (view.path === path && x > 7) return x;
      }
    }
    return null;
  }, { path, direction });

  for (const [key, direction, face] of [["KeyG", 1, "diff"], ["KeyB", -1, "blame"]]) {
    await page.keyboard.press(key, { delay: 150 });
    // The center file has a neighbor at 27 world units with a half-width of 7.
    await expect.poll(async () => (await sideOffset(direction)) ?? Infinity).toBeLessThan(20);
    if (face === "blame") await expect(page.locator("#blame")).toHaveAttribute("href", /\/src\/7\.rs#L\d+$/);
    await page.screenshot({ path: info.outputPath(`${face}-wall.png`) });
  }

  await canvas.click({ position: { x: 720, y: 600 }, button: "right", delay: 150 });
  await expect(canvas).toHaveAttribute("data-action-menu", /^Fly to face/);
  await page.keyboard.press("ArrowDown", { delay: 150 });
  await expect(canvas).toHaveAttribute("data-action-menu", /^Diff/);
  await page.keyboard.press("Enter", { delay: 150 });
  await expect.poll(async () => (await sideOffset(1)) ?? Infinity).toBeLessThan(20);
});

test("a plain click near the roof of a Ruby file returns to its code", async ({ page }, info) => {
  const revision = "c".repeat(40);
  const path = "test/encryption/message_test.rb";
  const source = `class MessageTest\n${"  assert message.encrypted_payload\n".repeat(41)}end\n`;
  const key = "repo-city-last-view-v1:" + JSON.stringify(["owner/roof", revision, null]);
  const roof = 14 * (Math.floor(source.length / 40) * 27 / 1536 + 0.055);
  await page.addInitScript(({ key, path, roof }) => {
    localStorage.setItem(key, JSON.stringify({ path, offset: [0, roof - 1, 15], rotation: [0, 0, 0, 1] }));
  }, { key, path, roof });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const api = new URL(route.request().url()).pathname;
    return route.fulfill({ json: api.endsWith("/repos/owner/roof") ? { default_branch: "main" }
      : api.includes("/commits/") ? { sha: revision, parents: [] }
      : api.includes("/git/trees/") ? { tree: [{ path, type: "blob", mode: "100644", sha: revision, size: source.length }] }
      : { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Froof");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas");
  const center = { x: 640, y: 400 };
  await canvas.focus();
  await canvas.evaluate(element => element.requestPointerLock());
  await page.keyboard.press("Space", { delay: 550 });
  await page.keyboard.press("KeyW", { delay: 1150 });
  await page.keyboard.press("ArrowDown", { delay: 1250 });
  await canvas.click({ position: center, button: "right", delay: 150 });
  await expect(canvas).toHaveAttribute("data-action-menu", /^Fly to face/);
  await canvas.click({ position: center, button: "right", delay: 150 });
  await expect(canvas).not.toHaveAttribute("data-action-menu");
  await page.evaluate(key => localStorage.removeItem(key), key);

  await canvas.click({ position: center, delay: 150 });
  const saved = () => page.evaluate(key => JSON.parse(localStorage.getItem(key)), key);
  await expect.poll(async () => (await saved())?.offset[2] ?? 0, { timeout: 12_000 }).toBeGreaterThan(7);
  expect(Math.abs((await saved()).rotation[0])).toBeLessThan(0.1);
  await page.screenshot({ path: info.outputPath("source-wall-after-roof-click.png") });
});
