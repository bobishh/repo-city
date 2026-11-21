import { test, expect } from "@playwright/test";

test("keeps the world selector behind the preview flag", async ({ page }) => {
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  const selector = page.locator(".world-picker");
  await page.goto("/");
  await expect(selector).toHaveJSProperty("hidden", true);
  await page.goto("/?world-picker=1");
  await expect(selector).toHaveJSProperty("hidden", false);
  await expect(selector.locator("option:checked")).toHaveText("Code City");
});

test("opens a repository in the Greek Library and unrolls a file at the requested line", async ({ page }, testInfo) => {
  const revision = "a".repeat(40);
  const document = "b".repeat(40);
  const source = Array.from({ length: 2400 }, (_, row) => `pub const ROW_${row + 1}: usize = ${row + 1};\n`).join("");
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/library") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: revision, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [
      { path: "README.md", type: "blob", mode: "100644", sha: revision, size: 9 },
      { path: "src/scroll.rs", type: "blob", mode: "100644", sha: document, size: source.length },
    ] } });
    const body = Buffer.from(path.endsWith(document) ? source : "# Library");
    return route.fulfill({ json: { size: body.length, encoding: "base64", content: body.toString("base64") } });
  });
  await page.goto("/?repo=owner%2Flibrary&world=greek-library");
  const canvas = page.locator("#repo-city");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect(canvas).toHaveAccessibleName("Greek Library");
  await expect(page).toHaveURL(/world=greek-library/);
  await expect(page.locator("#city-progress")).toBeHidden();
  await expect(page.locator(".wordmark")).toHaveCSS("font-family", /GFS Didot/);
  await canvas.focus();
  await page.screenshot({ path: testInfo.outputPath("library.png") });

  const press = code => page.keyboard.press(code, { delay: 135 });
  await press("Slash");
  await page.keyboard.type("no_such_file", { delay: 65 });
  await press("Enter");
  await expect(canvas).toHaveAccessibleName("Greek Library");
  await press("Slash");
  await page.keyboard.type("scroll.rs", { delay: 65 });
  await press("Enter");
  await expect(canvas).toHaveAccessibleName("Greek Library, src/scroll.rs, lines 1–22", { timeout: 30_000 });
  await page.screenshot({ path: testInfo.outputPath("unrolling.png") });

  const visibleInk = async () => {
    const png = await page.screenshot({ clip: { x: 225, y: 145, width: 250, height: 500 } });
    return page.evaluate(async encoded => {
      const blob = new Blob([Uint8Array.from(atob(encoded), char => char.charCodeAt(0))], { type: "image/png" });
      const bitmap = await createImageBitmap(blob);
      const context = new OffscreenCanvas(bitmap.width, bitmap.height).getContext("2d");
      context.drawImage(bitmap, 0, 0);
      const { data } = context.getImageData(0, 0, bitmap.width, bitmap.height);
      let ink = 0;
      for (let pixel = 0; pixel < data.length; pixel += 4) {
        if (data[pixel] < 180 && data[pixel + 2] > data[pixel + 1] + 8) ink++;
      }
      return ink;
    }, png.toString("base64"));
  };
  await expect.poll(visibleInk, { timeout: 7500 }).toBeGreaterThan(150);
  await canvas.focus();
  await press("KeyJ");
  await expect(canvas).toHaveAccessibleDescription("Enter a line number. Press Enter to go.");
  await page.keyboard.type("1750", { delay: 100 });
  await press("Enter");
  await expect(canvas).toHaveAccessibleName("Greek Library, src/scroll.rs, lines 1750–1771");
  await expect.poll(visibleInk, { timeout: 7500 }).toBeGreaterThan(150);
  await page.screenshot({ path: testInfo.outputPath("scroll.png") });
  await press("End");
  await expect(canvas).toHaveAccessibleName("Greek Library, src/scroll.rs, lines 2379–2400");
  await press("Escape");
  await expect(canvas).toHaveAccessibleName("Greek Library");
});
