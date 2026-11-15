import { test, expect } from "@playwright/test";
import { writeFile } from "node:fs/promises";

const after = "b".repeat(40);
const before = "a".repeat(40);

test("opens a public repository and fetches source after the WASM city starts", async ({ page }, testInfo) => {
  const faults = [];
  const consoleLog = [];
  page.on("pageerror", error => faults.push(error.message));
  page.on("console", message => {
    consoleLog.push(`${message.type()}: ${message.text()}`);
    if (message.type() === "error" || message.text().includes("DeviceLost")) {
      faults.push(message.text());
    }
  });
  await page.route("https://api.github.com/**", async route => {
    const path = new URL(route.request().url()).pathname;
    const source = [
      "fn demo() {",
      `    println!("source ${path.endsWith(after) ? "after" : "before"}");`,
      ...Array.from({ length: 20 }, (_, line) => `    let _row_${line} = ${line};`),
      "}",
      "",
    ].join("\n");
    if (path === "/repos/owner/demo") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [{ sha: before }] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: {
      tree: [{ path: "src/demo.rs", type: "blob", mode: "100644", sha: path.endsWith(after) ? after : before, size: source.length }],
    } });
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/");
  const cover = page.locator("#start-screen");
  await expect(cover.getByText("Repo City", { exact: true })).toBeVisible();
  await expect.poll(() => cover.locator("img").evaluate(image => image.naturalWidth)).toBeGreaterThan(0);
  await page.getByRole("textbox", { name: "Public GitHub repository" }).fill("owner/demo");
  const blob = page.waitForResponse(response => response.url().includes("/git/blobs/"), { timeout: 75_000 }).catch(() => null);
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.locator("#status")).toBeEmpty({ timeout: 70_000 });
  expect(await blob).not.toBeNull();
  await expect(page.locator("form")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas");
  await expect(canvas).toBeVisible();
  await expect(cover).toBeHidden();
  const cdp = await page.context().newCDPSession(page);
  const { x, y, width, height } = await canvas.boundingBox();
  let png;
  try {
    await expect.poll(async () => {
      expect(faults).toEqual([]);
      // Read the composited pixels without waiting for animation frames to check element stability.
      const { data } = await cdp.send("Page.captureScreenshot", {
        format: "png",
        clip: { x, y, width, height, scale: 1 },
        fromSurface: true,
      });
      png = Buffer.from(data, "base64");
      return page.evaluate(async data => {
        const image = await createImageBitmap(new Blob([Uint8Array.fromBase64(data)], { type: "image/png" }));
        const sample = new OffscreenCanvas(256, 128).getContext("2d");
        sample.drawImage(image, 0, 0, 256, 128);
        image.close();
        const { data: pixels } = sample.getImageData(0, 0, 256, 128);
        const stripeRows = new Set();
        for (let i = 0; i < pixels.length; i += 4) {
          const [red, green, blue] = pixels.subarray(i, i + 3);
          // The modification marker spans the file block vertically.
          if (red > 120 && red - green > 20 && green - blue > 25) {
            stripeRows.add(Math.floor(i / (256 * 4)));
          }
        }
        return stripeRows.size;
      }, data);
    }, { timeout: 100_000, message: "Render the modified file's vertical amber marker" }).toBeGreaterThan(50);
  } catch (error) {
    if (png) await writeFile(testInfo.outputPath("last-canvas.png"), png);
    await writeFile(testInfo.outputPath("browser-console.txt"), consoleLog.join("\n"));
    throw error;
  } finally {
    await cdp.detach();
  }
  expect(faults).toEqual([]);
});

test("Tab captures and Escape releases the browser pointer", async ({ page }) => {
  // Exercise the canvas handlers with a stand-in for the OS pointer in headless Chromium.
  await page.addInitScript(() => {
    let locked = null;
    Object.defineProperty(document, "pointerLockElement", { get: () => locked });
    HTMLCanvasElement.prototype.requestPointerLock = function () {
      locked = this;
      return Promise.resolve();
    };
    document.exitPointerLock = () => { locked = null; };
  });
  await page.goto("/");
  const canvas = page.locator("canvas");
  await canvas.dispatchEvent("keydown", { code: "Tab" });
  expect(await page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
  await canvas.dispatchEvent("keydown", { code: "Escape" });
  expect(await page.evaluate(() => document.pointerLockElement)).toBeNull();
});

test("reports missing repositories and keeps the form usable", async ({ page }) => {
  await page.route("https://api.github.com/**", route => route.fulfill({ status: 404 }));
  await page.goto("/");
  await page.getByRole("textbox", { name: "Public GitHub repository" }).fill("owner/missing");
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.getByRole("status")).toHaveText("Public repository, revision, or file not found.");
  await expect(page.getByRole("button", { name: "Open city" })).toBeEnabled();
  await expect(page.locator("#start-screen").getByText("Repo City", { exact: true })).toBeVisible();
});
