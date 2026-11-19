import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { gpuClick, expectOpen, expectClosed } from "./gpu-tablet.mjs";

for (const extension of ["jpeg", "JPG", "webp", "gif", "bmp", "ico", "tiff", "TGA", "pnm", "qoi", "dds", "hdr"]) test(`P renders ${extension} pixels in the GPU tablet`, async ({ page }, info) => {
  await page.addInitScript(() => {
    localStorage.setItem("repo-city-guide-v1", "seen");
    let locked = null;
    Object.defineProperty(document, "pointerLockElement", { get: () => locked });
    HTMLCanvasElement.prototype.requestPointerLock = function () { locked = this; queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange"))); return Promise.resolve(); };
    document.exitPointerLock = () => { locked = null; queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange"))); };
  });
  const data = ["jpeg", "JPG", "webp"].includes(extension) ? await page.evaluate(type => {
    const canvas = document.createElement("canvas"); canvas.width = 160; canvas.height = 100;
    const context = canvas.getContext("2d"); context.fillStyle = "rgb(210,40,170)"; context.fillRect(0, 0, 160, 100);
    return canvas.toDataURL(type).split(",")[1];
  }, extension === "webp" ? "image/webp" : "image/jpeg")
    : (await readFile(new URL(`./fixtures/image-codecs/raster.${extension.toLowerCase()}`, import.meta.url))).toString("base64");
  const bytes = Buffer.from(data, "base64"), head = "b".repeat(40), name = `photo.${extension}`;
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    return route.fulfill({ json: path === "/repos/owner/images" ? { default_branch: "main" }
      : path.includes("/commits/") ? { sha: head, parents: [] }
      : path.includes("/git/trees/") ? { tree: [{ path: name, type: "blob", mode: "100644", sha: head, size: bytes.length }] }
      : path.includes("/git/blobs/") ? { size: bytes.length, encoding: "base64", content: data } : [] });
  });
  await page.goto("/?repo=owner%2Fimages");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.locator("canvas").focus(); await page.keyboard.press("Meta+p", { delay: 120 });
  const pad = page.locator("#review-dialog");
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name, exact: true }));
  await expectClosed(page); await page.waitForTimeout(1500);
  await page.locator("canvas").evaluate(canvas => canvas.requestPointerLock());
  await page.keyboard.press("KeyP", { delay: 180 }); await expectOpen(page, name);
  await expect(pad.getByRole("img", { includeHidden: true, name })).toBeAttached();
  // Verify Bevy canvas pixels, rather than the hidden DOM image service alone.
  await expect.poll(async () => {
    const screenshot = await page.screenshot();
    return page.evaluate(async bytes => {
      const bitmap = await createImageBitmap(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
      const canvas = document.createElement("canvas"); canvas.width = bitmap.width; canvas.height = bitmap.height;
      const context = canvas.getContext("2d"); context.drawImage(bitmap, 0, 0); bitmap.close();
      const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
      let matches = 0;
      // The physical tablet's color transform changes intensity; the image keeps its magenta chroma.
      for (let i = 0; i < pixels.length; i += 4) if (pixels[i] > 120 && pixels[i + 2] > 100 && pixels[i + 1] < 100 && pixels[i] > pixels[i + 1] * 1.7 && pixels[i + 2] > pixels[i + 1] * 1.5) matches++;
      return matches;
    }, [...screenshot]);
  }, { timeout: 15_000 }).toBeGreaterThan(10_000);
  await page.screenshot({ path: info.outputPath("image-preview.png") });
});
