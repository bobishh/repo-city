import { test, expect } from "@playwright/test";
import { gpuSelect } from "./gpu-tablet.mjs";

const state = page => page.evaluate(() => globalThis.repoCity?.readTablet());
const text = view => JSON.stringify(view?.nodes ?? []);
async function clickControl(page, name) {
  await expect.poll(async () => (await state(page))?.layout.controls.some(control => control.text === name && control.visible && control.order >= 0)).toBe(true);
  const point = await page.evaluate(name => {
    const layout = globalThis.repoCity.readTablet().layout;
    const { bounds } = layout.controls.find(control => control.text === name && control.visible && control.order >= 0);
    const canvas = document.querySelector("canvas").getBoundingClientRect();
    return [canvas.x + (bounds[0] + bounds[2] / 2) * canvas.width / layout.viewport[0], canvas.y + (bounds[1] + bounds[3] / 2) * canvas.height / layout.viewport[1]];
  }, name);
  await page.mouse.click(...point, { delay: 100 });
}
async function renderedHeading(page, title) {
  await expect.poll(async () => {
    const view = await state(page);
    const heading = view?.layout.controls.find(control => control.kind === "heading" && control.text === title && control.visible);
    if (!heading) return 0;
    const png = (await page.screenshot()).toString("base64");
    return page.evaluate(async ({ png, bounds, viewport }) => {
      const bytes = Uint8Array.from(atob(png), character => character.charCodeAt(0));
      const image = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const scale = image.width / viewport[0];
      const sample = new OffscreenCanvas(160, 40).getContext("2d");
      sample.drawImage(image, bounds[0] * scale, bounds[1] * scale, bounds[2] * scale, bounds[3] * scale, 0, 0, 160, 40);
      image.close();
      const pixels = sample.getImageData(0, 0, 160, 40).data;
      let ink = 0;
      for (let index = 0; index < pixels.length; index += 4) {
        const [red, green, blue] = pixels.subarray(index, index + 3);
        if (green > 100 && green > red * 1.15 && blue > 80) ink++;
      }
      return ink;
    }, { png, bounds: heading.bounds, viewport: view.layout.viewport });
  }, { timeout: 60_000, message: `GPU renders the ${title} heading` }).toBeGreaterThan(30);
}
async function repository(page) {
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    const source = "fn main() { println!(\"Hello\"); }\n";
    return route.fulfill({ json: path === "/repos/owner/gpu" ? { default_branch: "main" }
      : path.includes("/commits/") ? { sha: "b".repeat(40), parents: [] }
      : path.includes("/git/trees/") ? { tree: ["main.rs", "second.rs"].map(name => ({ path: name, type: "blob", mode: "100644", sha: "b".repeat(40), size: source.length })) }
      : { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Fgpu");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.locator("canvas").focus();
}

test("GPU tablet renders help and autosaved settings, then finds a file with keyboard only", async ({ page }, testInfo) => {
  const faults = []; page.on("pageerror", error => faults.push(error.message));
  await repository(page);
  await page.keyboard.press("h", { delay: 180 });
  await expect.poll(async () => (await state(page))?.title).toBe("Help");
  expect((await state(page)).layout.viewport).toEqual([page.viewportSize().width, page.viewportSize().height]);
  await expect(page.locator("#review-dialog")).toBeHidden();
  await expect.poll(async () => (await state(page))?.layout.controls.some(control => control.text === "Start tour")).toBe(true);
  await renderedHeading(page, "Help");
  await page.screenshot({ path: testInfo.outputPath("gpu-help.png") });
  await clickControl(page, "Settings");
  await expect.poll(async () => (await state(page))?.title).toBe("Settings");
  expect((await state(page)).layout.controls.some(control => /Save settings/.test(control.text))).toBe(false);
  await clickControl(page, "Minimum delay (seconds)");
  await page.keyboard.press("ControlOrMeta+A", { delay: 180 }); await page.keyboard.type("9", { delay: 90 });
  await expect.poll(async () => text(await state(page))).toContain("Minimum delay must not exceed maximum delay.");
  await page.keyboard.press("ControlOrMeta+A", { delay: 180 }); await page.keyboard.type("3", { delay: 90 });
  await expect.poll(async () => text(await state(page))).toContain("Saved automatically.");
  await renderedHeading(page, "Settings");
  await page.screenshot({ path: testInfo.outputPath("gpu-settings.png") });
  await page.keyboard.press("Alt+2", { delay: 180 });
  await expect.poll(async () => (await state(page))?.title).toBe("Find a file");
  await page.keyboard.type("second", { delay: 90 });
  await expect.poll(async () => text(await state(page))).toContain('"value":"second"');
  await page.keyboard.press("Enter", { delay: 180 });
  await expect.poll(async () => (await state(page))?.open).toBe(false);
  await page.keyboard.press("h", { delay: 180 });
  await expect.poll(async () => (await state(page))?.open).toBe(true);
  await page.keyboard.press("h", { delay: 180 });
  await expect.poll(async () => (await state(page))?.open).toBe(false);
  expect(faults).toEqual([]);
});

test("radio station saves from GPU settings while city ambience remains off", async ({ page }) => {
  await repository(page);
  await page.keyboard.press("h", { delay: 180 });
  await clickControl(page, "Settings");
  const radio = page.locator("#review-dialog").getByLabel("Radio");
  await expect(radio).toHaveValue("off");
  await gpuSelect(page, radio, "space_trance");
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("repo-city-audio-v1"))))
    .toMatchObject({ ambience: false, radio: "space_trance" });
  await gpuSelect(page, radio, "off");
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("repo-city-audio-v1"))).then(value => value.radio)).toBe("off");
});
