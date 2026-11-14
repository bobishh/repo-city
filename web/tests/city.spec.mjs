import { test, expect } from "@playwright/test";

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
    if (path === "/repos/owner/demo") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [{ sha: before }] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: {
      tree: [{ path: "src/demo.rs", type: "blob", mode: "100644", sha: path.endsWith(after) ? after : before, size: 64 }],
    } });
    const source = `fn demo() {\n    println!("source ${path.endsWith(after) ? "after" : "before"}");\n}\n`;
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/");
  await page.getByRole("textbox", { name: "Public GitHub repository" }).fill("owner/demo");
  const blob = page.waitForResponse(response => response.url().includes("/git/blobs/"), { timeout: 75_000 }).catch(() => null);
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.locator("#status")).toBeEmpty({ timeout: 70_000 });
  expect(await blob).not.toBeNull();
  await expect(page.locator("form")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas");
  await expect(canvas).toBeVisible();
  let png;
  try {
    await expect.poll(async () => {
      expect(faults).toEqual([]);
      png = await canvas.screenshot({ timeout: 20_000 });
      return page.evaluate(async data => {
        const image = await createImageBitmap(new Blob([Uint8Array.fromBase64(data)], { type: "image/png" }));
        const sample = new OffscreenCanvas(64, 64).getContext("2d");
        sample.drawImage(image, 0, 0, 64, 64);
        image.close();
        const { data: pixels } = sample.getImageData(0, 0, 64, 64);
        let roofPixels = 0;
        for (let i = 0; i < pixels.length; i += 4) {
          const [red, green, blue] = pixels.subarray(i, i + 3);
          // The modified file has an amber roof. Neither the page nor the HUD uses that color.
          if (red > 120 && red - green > 20 && green - blue > 25) roofPixels++;
        }
        return roofPixels;
      }, png.toString("base64"));
    }, { timeout: 100_000, message: "Render the modified file's amber roof" }).toBeGreaterThan(12);
  } catch (error) {
    if (png) await testInfo.attach("last canvas", { body: png, contentType: "image/png" });
    await testInfo.attach("browser console", { body: consoleLog.join("\n"), contentType: "text/plain" });
    throw error;
  }
  expect(faults).toEqual([]);
});

test("reports missing repositories and keeps the form usable", async ({ page }) => {
  await page.route("https://api.github.com/**", route => route.fulfill({ status: 404 }));
  await page.goto("/");
  await page.getByRole("textbox", { name: "Public GitHub repository" }).fill("owner/missing");
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.getByRole("status")).toHaveText("Public repository, revision, or file not found.");
  await expect(page.getByRole("button", { name: "Open city" })).toBeEnabled();
});
