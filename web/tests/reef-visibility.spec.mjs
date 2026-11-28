import { test, expect } from "@playwright/test";

test("distant stations stay visible when an arrow leaves the Repo Reef overview", async ({ page }, info) => {
  const revision = "b".repeat(40);
  const source = "package reef\n";
  const tree = Array.from({ length: 16_572 }, (_, index) => ({
    path: `district-${String(Math.floor(index / 144)).padStart(3, "0")}/station-${String(index % 144).padStart(3, "0")}.go`,
    type: "blob", mode: "100644", sha: revision, size: source.length,
  }));
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    return route.fulfill({ json: path === "/repos/owner/reef" ? { default_branch: "main" }
      : path.includes("/commits/") ? { sha: revision, parents: [] }
      : path.includes("/git/trees/") ? { tree }
      : { encoding: "base64", content: Buffer.from(source).toString("base64"), size: source.length } });
  });

  await page.goto("/?world=repo-reef&repo=owner%2Freef");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("#repo-city");
  const hint = page.locator("#city-hint");
  async function edges(filename) {
    const png = await canvas.screenshot(filename ? { path: info.outputPath(filename) } : {});
    return page.evaluate(async encoded => {
      const bytes = Uint8Array.from(atob(encoded), char => char.charCodeAt(0));
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const { width, height } = bitmap;
      const ctx = new OffscreenCanvas(width, height).getContext("2d");
      ctx.drawImage(bitmap, 0, 0);
      bitmap.close();
      const { data } = ctx.getImageData(0, 0, width, height);
      // Station roofs make repeated sharp edges where the old haze made a smooth gradient.
      let count = 0;
      for (let y = Math.round(height * 0.28); y < height * 0.72; y += 2) {
        for (let x = Math.round(width * 0.15); x < width * 0.85; x += 2) {
          const current = (y * width + x) * 4;
          if (Math.abs(data[current + 1] - data[current + 9]) >= 12) count++;
        }
      }
      return count;
    }, png.toString("base64"));
  }
  await canvas.focus();
  await page.keyboard.press("KeyM", { delay: 140 });
  await expect(hint).toContainText("M shows the whole project");
  await page.keyboard.press("KeyM", { delay: 140 });
  await expect(hint).toContainText("Click a district");
  await expect.poll(() => edges(), { timeout: 20_000 }).toBeGreaterThan(250);
  await canvas.screenshot({ path: info.outputPath("project-overview.png") });

  await page.keyboard.press("ArrowLeft", { delay: 140 });
  await expect(canvas).not.toHaveAttribute("data-overview", "");
  await expect.poll(() => edges(), { timeout: 10_000 }).toBeGreaterThan(250);
  await edges("clear-water.png");
});
