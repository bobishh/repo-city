import { test, expect } from "@playwright/test";
import { readFile, writeFile } from "node:fs/promises";
import { readTablet, expectOpen, expectClosed, gpuClick, gpuFill, gpuSelect, gpuFocus } from "./gpu-tablet.mjs";

const after = "b".repeat(40);
const before = "a".repeat(40);
const next = "e".repeat(40);

test("opens a rendered Markdown document with P and follows its relative link", async ({ page }) => {
  await emulatePointerLock(page);
  const license = "5".repeat(40);
  const licenseText = "Permission is hereby granted to use this source.\n";
  const source = "# Project notes\n\nSee the [MIT License](docs/LICENSE).\n\nUse **this** command.\n\n- First step\n\n| Action | Example |\n| --- | --- |\n| Inspect | status |\n\n```sh\ngit status\n```\n\n[Unsafe](javascript:alert%281%29)\n\n" +
    "Read the next paragraph.\n\n".repeat(50);
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/preview") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: {
      tree: [
        { path: "README.md", type: "blob", mode: "100644", sha: after, size: source.length },
        { path: "docs/LICENSE", type: "blob", mode: "100644", sha: license, size: licenseText.length },
      ],
    } });
    const body = path.endsWith(license) ? licenseText : source;
    return route.fulfill({ json: { size: body.length, encoding: "base64", content: Buffer.from(body).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Fpreview");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas");
  await page.mouse.move(640, 400);
  await canvas.evaluate(element => element.requestPointerLock());
  await expect(canvas).toHaveAttribute("data-pointer-locked", "");
  await expect(page.locator("#progress-summary")).toContainText("0 lines revealed");
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.keyboard.press("KeyP", { delay: 180 });
  const preview = page.locator("#review-dialog");
  await expectOpen(page, "README.md");
  await expect(preview.locator("[data-markdown-source]")).toHaveAttribute("data-markdown-source", source);
  await expect(preview.locator('a[href^="javascript:"]')).toHaveCount(0);
  await expect(page.locator("#progress-summary")).toContainText("0 lines revealed");
  await page.mouse.move(640, 400);
  await page.mouse.wheel(0, 1000);
  await expect(page.locator("#progress-summary")).toContainText(`${source.split("\n").length - 1} lines revealed`, { timeout: 20_000 });
  await page.mouse.wheel(0, -1000);
  let point;
  await expect.poll(async () => {
    const screenshot = await page.screenshot({ scale: "css" });
    const coloredPixel = await page.evaluate(async png => {
      const bytes = Uint8Array.from(atob(png), byte => byte.charCodeAt(0));
      const image = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const context = new OffscreenCanvas(image.width, image.height).getContext("2d");
      context.drawImage(image, 0, 0);
      const { data, width } = context.getImageData(0, 0, image.width, image.height);
      for (let y = 230; y < 520; y++) {
        for (let x = 165; x < 440; x++) {
          const i = (y * width + x) * 4;
          const [red, green, blue] = [data[i], data[i + 1], data[i + 2]];
          if (blue > 150 && blue > green + 45 && green > red + 30) return [x, y];
        }
      }
      return null;
    }, screenshot.toString("base64"));
    const settled = coloredPixel && point && coloredPixel.every((value, index) => value === point[index]);
    point = coloredPixel;
    return Boolean(settled);
  }).toBe(true);
  await page.mouse.click(...point, { delay: 100 });
  await expectOpen(page, "docs/LICENSE");
  await expect(preview.locator(".plain-file-preview")).toHaveText(licenseText.trim());
  await page.keyboard.press("Tab", { delay: 120 });
  await expectClosed(page);
});

test("opens under the Repo City Web name with the city artwork", async ({ page }, testInfo) => {
  await page.goto("/");
  await expect(page).toHaveTitle("Repo City Web");
  await expect(page.getByRole("button", { name: "Repo City Web", exact: true })).toBeVisible();
  await expect(page.locator("#logo .wordmark-edition")).toBeVisible();
  const cover = page.locator("#start-screen img");
  await expect(cover).toHaveAttribute("src", "./assets/repo-city-start.webp");
  await expect.poll(() => cover.evaluate(image => image.naturalWidth)).toBeGreaterThan(1000);
  await expect(page.getByRole("combobox", { name: "GitHub repository" })).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: testInfo.outputPath("repo-city-web.png") });
});

test("shows the guide once and keeps it available from the help button", async ({ page }, testInfo) => {
  const faults = []; page.on("pageerror", error => faults.push(error.message));
  let releaseTree;
  const pendingTree = new Promise(resolve => { releaseTree = resolve; });
  const source = "fn demo() {}\n";
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/demo") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [] } });
    if (path.includes("/git/trees/")) {
      await pendingTree;
      return route.fulfill({ json: {
        tree: [{ path: "src/demo.rs", type: "blob", mode: "100644", sha: after, size: source.length }],
      } });
    }
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/");
  await expect(page.getByRole("link", { name: "Source on GitHub" })).toHaveAttribute("href", "https://github.com/bobishh/repo-city");
  await expect(page.getByRole("link", { name: "Build for Mac" })).toHaveCount(0);
  const guide = page.locator("#review-dialog");
  const help = page.getByRole("button", { name: "How to use Repo City" });
  await expectClosed(page);
  await expect(help).toBeHidden();
  await page.getByRole("combobox", { name: "GitHub repository" }).fill("owner/demo");
  await page.getByRole("button", { name: "Select repository" }).click();
  await page.getByRole("button", { name: "Open city" }).click();
  try {
    await expect(page.locator("#status")).toContainText("Loading repository");
    await expectClosed(page);
    await expect(help).toBeHidden();
  } finally {
    releaseTree();
  }
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect(page.locator("canvas")).toBeVisible();
  expect(await page.evaluate(() => globalThis.repoCity.reviewCanPaint())).toBe(false);
  await expectOpen(page);
  await expect(guide.getByText("Click the Repo City logo to open the repository picker.")).toBeAttached();
  await gpuClick(page, guide.getByRole("button", { includeHidden: true, name: "Skip", exact: true }));
  await expectClosed(page);
  await page.reload();
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expectClosed(page);
  await help.click();
  await expectOpen(page);
  const cdpTablet = await page.context().newCDPSession(page);
  const imageTablet = await cdpTablet.send("Page.captureScreenshot", { format: "png", fromSurface: true });
  await writeFile(testInfo.outputPath("vibepad-help.png"), Buffer.from(imageTablet.data, "base64"));
  await cdpTablet.detach();
  await gpuClick(page, guide.getByRole("button", { includeHidden: true, name: "Settings" }));
  const settings = page.locator("#review-dialog");
  await expect(settings).toContainText("Brysbaert");
  await expect(settings.getByLabel("Lines per wash block")).toHaveValue("6");
  await gpuFill(page, settings.getByLabel("Lines per wash block"), "0");
  await expect(settings.locator(".review-reading [role=status]")).toContainText("positive whole number");
  await gpuFill(page, settings.getByLabel("Lines per wash block"), "1");
  await expect(settings.getByLabel("Minimum delay (seconds)")).toHaveValue("3");
  await gpuFill(page, settings.getByLabel("Minimum delay (seconds)"), "7");
  await expect(settings.locator(".review-reading [role=status]")).toContainText("Minimum delay must not exceed");
  await gpuFill(page, settings.getByLabel("Minimum delay (seconds)"), "");
  await gpuFill(page, settings.getByLabel("Maximum delay (seconds)"), "5.5");
  await expect(settings.locator(".review-reading [role=status]")).toContainText("Saved");
  await gpuSelect(page, settings.getByLabel("Reveal"), "immediate");
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("repo-city-reading-v1"))))
    .toEqual({mode: "immediate", block_kind: "function", auto_advance: true, block_lines: 1, min_seconds: null, max_seconds: 5.5});
  await page.keyboard.press("Escape", { delay: 120 });
  await expectClosed(page);
  await page.evaluate(() => localStorage.removeItem("repo-city-guide-v1"));
  await page.reload();
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await gpuClick(page, guide.getByRole("button", { includeHidden: true, name: "Start tour" }));
  await expectClosed(page);
  expect(await page.evaluate(() => globalThis.repoCity.tourCommand() & 1)).toBe(1);
  expect(faults).toEqual([]);
});

test.beforeEach(async ({ page }, testInfo) => {
  if (!testInfo.title.startsWith("shows the guide once")) {
    await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  }
});

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
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: path.endsWith(next) ? next : after, parents: [{ sha: before }] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: {
      tree: [{ path: "src/demo.rs", type: "blob", mode: "100644", sha: path.split("/").at(-1), size: source.length }],
    } });
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/");
  const cover = page.locator("#start-screen");
  await expect(page.getByRole("button", { name: "Repo City Web", exact: true })).toBeVisible();
  await expect(page.getByRole("combobox", { name: "GitHub repository" })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("landing.png") });
  const syntax = page.waitForResponse(response => new URL(response.url()).pathname === "/syntax-worker.mjs");
  await expect.poll(() => cover.locator("img").evaluate(image => image.naturalWidth)).toBeGreaterThan(0);
  await page.getByRole("combobox", { name: "GitHub repository" }).fill("owner/demo");
  await page.getByRole("button", { name: "Select repository" }).click();
  const blob = page.waitForResponse(response => response.url().includes("/git/blobs/"), { timeout: 75_000 }).catch(() => null);
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.locator("#status")).toBeEmpty({ timeout: 70_000 });
  expect(await blob).not.toBeNull();
  await expect(page.locator("form")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas");
  await expect(canvas).toBeVisible();
  await expect(cover).toBeHidden();
  await expect.poll(() => canvas.evaluate(element => ({
    top: element.getBoundingClientRect().top,
    scroll: window.scrollY,
  }))).toEqual({ top: 0, scroll: 0 });
  expect(await syntax).not.toBeNull();
  await expect.poll(() => page.evaluate(() => globalThis.repoCity.syntaxVersion()), { timeout: 30_000 }).toBeGreaterThan(0);
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
    await expect.poll(async () => {
      const shot = await cdp.send("Page.captureScreenshot", { format: "png", fromSurface: true });
      return page.evaluate(async data => {
      const image = await createImageBitmap(new Blob([Uint8Array.fromBase64(data)], { type: "image/png" }));
      const head = new OffscreenCanvas(48, 48).getContext("2d");
      const scale = image.width / innerWidth;
      head.drawImage(image, 16 * scale, 16 * scale, 48 * scale, 48 * scale, 0, 0, 48, 48);
      image.close();
      const { data: pixels } = head.getImageData(0, 0, 48, 48);
      let metal = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        const [red, green, blue] = pixels.subarray(i, i + 3);
        if (red > 70 && green > 60 && red > blue + 10) metal++;
      }
      return metal;
    }, shot.data);
    }, { timeout: 30_000, message: "WebGPU renders the android in the logo" }).toBeGreaterThan(30);
    await writeFile(testInfo.outputPath("city.png"), png);
  } catch (error) {
    if (png) await writeFile(testInfo.outputPath("last-canvas.png"), png);
    await writeFile(testInfo.outputPath("browser-console.txt"), consoleLog.join("\n"));
    throw error;
  } finally {
    await cdp.detach();
  }
  expect(faults).toEqual([]);
  // The overview uses a free cursor without blocking the normal Help shortcut.
  await page.mouse.move(640, 400);
  await canvas.focus();
  await page.keyboard.press("KeyM", { delay: 180 });
  await expect(canvas).toHaveAttribute("data-overview", "");
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.keyboard.press("KeyM", { delay: 180 });
  await expect(canvas).not.toHaveAttribute("data-pointer-ui", "");
  await page.keyboard.press("KeyH", { delay: 180 });
  const mapHelp = page.locator("#review-dialog");
  await expectOpen(page);
  await page.keyboard.press("Escape", { delay: 120 });
  await expectClosed(page);
  await canvas.focus();
  await page.keyboard.press("KeyM", { delay: 180 });
  await expect(canvas).not.toHaveAttribute("data-overview", "");
  await page.getByRole("button", { name: "Repo City", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Open repository" });
  await expect(dialog).toBeVisible();
  await expect(canvas).toBeVisible();
  await page.route("https://api.github.com/repos/owner/missing", route => route.fulfill({ status: 404 }));
  await dialog.getByRole("button", { name: "Change repository" }).click();
  await dialog.getByRole("combobox", { name: "GitHub repository" }).fill("owner/missing");
  await page.getByRole("button", { name: "Select repository" }).click();
  await dialog.getByRole("button", { name: "Open city" }).click();
  await expect(dialog.getByRole("status")).toHaveText("Repository, revision, or file not found. Private repositories need GitHub access.");
  await expect(canvas).toBeVisible();
  await dialog.getByRole("button", { name: "Change repository" }).click();
  await dialog.getByRole("combobox", { name: "GitHub repository" }).fill("owner/demo");
  await page.getByRole("button", { name: "Select repository" }).click();
  await dialog.getByRole("button", { name: "Revision: Default branch" }).click();
  await dialog.getByRole("combobox", { name: "Find a branch, tag, or commit" }).fill(next);
  await dialog.getByRole("button", { name: "Use revision" }).click();
  await expect(page.locator('input[name="ref"]')).toHaveValue(next);
  await dialog.getByRole("button", { name: "Open city" }).click();
  await expect.poll(() => page.evaluate(() => JSON.parse(repoCity.snapshot()).after["src/demo.rs"]?.id)).toBe(next);
  await expect(dialog).toBeHidden();
  await expect(page).toHaveURL(new RegExp("ref=" + next));
  await page.getByRole("button", { name: "Repo City", exact: true }).click();
  await page.keyboard.press("Escape", { delay: 120 });
  await expect(dialog).toBeHidden();
});

async function emulatePointerLock(page) {
  // Headless Chromium does not reliably expose OS pointer lock to Playwright.
  await page.addInitScript(() => {
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
}

test("the first canvas click captures the pointer and Escape releases it", async ({ page }) => {
  await emulatePointerLock(page);
  await page.goto("/");
  const canvas = page.locator("canvas");
  await canvas.evaluate(element => { element.hidden = false; });
  await canvas.click({ position: { x: 400, y: 300 } });
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
  await expect(canvas).toHaveCSS("cursor", "none");
  await page.keyboard.press("Escape", { delay: 120 });
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id ?? null)).toBeNull();
  await expect(canvas).not.toHaveCSS("cursor", "none");
  await canvas.evaluate(element => {
    const request = element.requestPointerLock;
    element.requestPointerLock = function () {
      this.requestPointerLock = request;
      // Simulate a browser request that disappears without an event or promise.
    };
  });
  await canvas.click({ position: { x: 400, y: 300 } });
  expect(await page.evaluate(() => document.pointerLockElement)).toBeNull();
  await canvas.evaluate(element => element.setAttribute("data-pointer-ui", ""));
  await canvas.click({ position: { x: 400, y: 300 } });
  expect(await page.evaluate(() => document.pointerLockElement)).toBeNull();
  await canvas.evaluate(element => element.removeAttribute("data-pointer-ui"));
  await canvas.click({ position: { x: 400, y: 300 } });
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
});

test("reports missing repositories and keeps the form usable", async ({ page }) => {
  await page.addInitScript(() => localStorage.removeItem("repo-city-guide-v1"));
  await page.route("https://api.github.com/**", route => route.fulfill({ status: 404 }));
  await page.goto("/");
  await page.getByRole("combobox", { name: "GitHub repository" }).fill("owner/missing");
  await page.getByRole("button", { name: "Select repository" }).click();
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.getByRole("status")).toHaveText("Repository, revision, or file not found. Private repositories need GitHub access.");
  await expect(page.getByRole("button", { name: "Open city" })).toBeEnabled();
  await expect(page.getByRole("button", { name: "Repo City Web", exact: true })).toBeVisible();
  await expect(page.getByRole("dialog", { name: "Help" })).toBeHidden();
  await expect(page.getByRole("button", { name: "How to use Repo City" })).toBeHidden();
});

test("opens the city while an older tab blocks the progress database upgrade", async ({ page, context }) => {
  const legacy = await context.newPage();
  await legacy.route("**/legacy-progress", route => route.fulfill({ contentType: "text/html", body: "<title>Old tab</title>" }));
  await legacy.goto("/legacy-progress");
  const scope = JSON.stringify(["owner/demo", after, null]);
  const mark = '{"reviewed":"src/demo.rs"}';
  await legacy.evaluate(async ({ scope, mark }) => {
    globalThis.legacyProgress = await new Promise((resolve, reject) => {
      const request = indexedDB.open("repo-city-progress", 1);
      request.onupgradeneeded = () => request.result.createObjectStore("marks");
      request.onerror = () => reject(request.error);
      request.onsuccess = () => resolve(request.result);
    });
    await new Promise((resolve, reject) => {
      const tx = globalThis.legacyProgress.transaction("marks", "readwrite");
      tx.objectStore("marks").put(mark, [scope, mark]);
      tx.oncomplete = resolve;
      tx.onabort = () => reject(tx.error);
    });
  }, { scope, mark });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    const source = "fn demo() {}\n";
    if (path === "/repos/owner/demo") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: {
      tree: [{ path: "src/demo.rs", type: "blob", mode: "100644", sha: after, size: source.length }],
    } });
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Fdemo");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 20_000 });
  await expect(page.locator("#progress-warning")).toContainText("Another Repo City Web tab is blocking the progress update.");
  expect(await legacy.evaluate(() => globalThis.legacyProgress.version)).toBe(1);
  await legacy.close();
  await expect.poll(() => page.evaluate(async key => {
    const { readProgress } = await import("/progress.mjs");
    return readProgress(key).catch(() => []);
  }, scope)).toEqual([mark]);
});

test("opens a merged pull request from the URL using its fork head and merge base", async ({ page }, testInfo) => {
  await emulatePointerLock(page);
  const checked = [];
  const blameTargets = [];
  await page.route(/\/github\/blame\?/, async route => {
    blameTargets.push(Object.fromEntries(new URL(route.request().url()).searchParams));
    return route.fulfill({ json: { ranges: [{
      startingLine: 1, endingLine: 1,
      commit: { abbreviatedOid: "b0123456", committedDate: "2026-11-15T14:30:00Z", author: { name: "Ada" } },
    }] } });
  });
  await page.route("https://api.github.com/**", async route => {
    const path = new URL(route.request().url()).pathname;
    checked.push(path);
    if (path.endsWith("/pulls/8")) return route.fulfill({ json: {
      state: "closed", merged: true,
      base: { sha: "c".repeat(40) },
      head: { sha: after, repo: { full_name: "fork/demo" } },
    } });
    if (path.includes("/compare/")) return route.fulfill({ json: { merge_base_commit: { sha: before } } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [
      { path: "src/pr.rs", type: "blob", mode: "100644", sha: path.endsWith(after) ? after : before, size: 18 },
      { path: "LICENSE", type: "blob", mode: "100644", sha: "d".repeat(40), size: 16 },
    ] } });
    const source = path.endsWith(after) ? "fn new_version() {}" : "fn old_version() {}";
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/?pr=https%3A%2F%2Fgithub.com%2Fowner%2Fdemo%2Fpull%2F8");
  await expect(page.locator("form")).toBeHidden({ timeout: 90_000 });
  const snapshot = await page.evaluate(() => JSON.parse(globalThis.repoCity.snapshot()));
  expect(snapshot).toMatchObject({ name: "owner/demo#8", revision: after, baseline: before });
  expect(Object.keys(snapshot.after)).toContain("LICENSE");
  expect(await page.evaluate(() => globalThis.repoCity.reviewCanPaint())).toBe(false);
  expect(checked).toContain(`/repos/fork/demo/git/trees/${after}`);
  expect(checked).toContain(`/repos/owner/demo/git/trees/${before}`);
  await page.locator("canvas").focus();
  await page.keyboard.press("Tab", { delay: 120 });
  const tablet = page.locator("#review-dialog");
  await expect(tablet.getByRole("button", { includeHidden: true, name: "src/pr.rs" })).toBeAttached();
  const cdpTablet = await page.context().newCDPSession(page);
  const imageTablet = await cdpTablet.send("Page.captureScreenshot", { format: "png", fromSurface: true });
  await writeFile(testInfo.outputPath("vibepad-files.png"), Buffer.from(imageTablet.data, "base64"));
  await cdpTablet.detach();
  await expect(tablet.getByRole("button", { includeHidden: true, name: "Settings", exact: true })).toBeAttached();
  await expect(tablet.locator('nav button', { hasText: "Settings" })).toHaveCount(0);
  await expect(tablet.getByRole("button", { includeHidden: true, name: "LICENSE" })).toHaveCount(0);
  await gpuClick(page, tablet.getByRole("button", { includeHidden: true, name: "Viewed", exact: true }));
  await expect(tablet).toContainText("No changed files viewed.");
  await expect(tablet.getByRole("button", { includeHidden: true, name: "Close", exact: true })).toHaveCount(0);
  await page.keyboard.press("Shift+Tab", { delay: 120 });
  await expectOpen(page);
  await page.keyboard.press("Tab", { delay: 120 });
  await expectClosed(page);
  await expect(page.locator("canvas")).toBeFocused();
  await page.keyboard.press("Tab", { delay: 120 });
  await expectOpen(page);
  await page.keyboard.press("Escape", { delay: 120 });
  await expectClosed(page);
  const blame = page.getByRole("link", { name: "Blame on GitHub" });
  await expect(blame).toBeHidden();
  const canvas = page.locator("canvas");
  await canvas.focus();
  await page.keyboard.press("b", { delay: 180 });
  await expect(blame).toBeVisible({ timeout: 55_000 });
  await expect(blame).toHaveAttribute("href", new RegExp(
    `https://github.com/fork/demo/blame/${after}/src/pr\\.rs#L1$`));
  await expect.poll(() => blameTargets).toEqual([{ owner: "fork", repo: "demo", sha: after, path: "src/pr.rs" }]);
  await expect.poll(() => page.evaluate(() => globalThis.repoCity.readBlame("src/pr.rs", false, 1)))
    .toBe('{"notes":["b0123456  2026-11-15 14:30  Ada"]}');
  await canvas.screenshot({ path: testInfo.outputPath("blame-authorship.png") });
  await canvas.focus();
  await page.keyboard.press("g", { delay: 180 });
  await expect(blame).toBeHidden({ timeout: 25_000 });
  const cdp = await page.context().newCDPSession(page);
  const { x, y, width, height } = await canvas.boundingBox();
  let screenshot;
  try {
    await expect.poll(async () => {
      const { data } = await cdp.send("Page.captureScreenshot", {
        format: "png", clip: { x, y, width, height, scale: 1 }, fromSurface: true,
      });
      screenshot = Buffer.from(data, "base64");
      return page.evaluate(async data => {
        const image = await createImageBitmap(new Blob([Uint8Array.fromBase64(data)], { type: "image/png" }));
        const sample = new OffscreenCanvas(256, 128).getContext("2d");
        sample.drawImage(image, 0, 0, 256, 128);
        const original = new OffscreenCanvas(image.width, image.height).getContext("2d");
        original.drawImage(image, 0, 0);
        const ink = original.getImageData(
          image.width * .075, image.height * .41, image.width * .24, image.height * .17,
        ).data;
        image.close();
        let keyword = 0;
        for (let i = 0; i < ink.length; i += 4) {
          const [red, green, blue] = ink.subarray(i, i + 3);
          if (blue > 130 && blue > red + 10 && blue > green + 30 && red > green + 14) keyword++;
        }
        const { data: pixels } = sample.getImageData(17, 45, 189, 55);
        let removal = 0, addition = 0;
        for (let i = 0; i < pixels.length; i += 4) {
          const [red, green, blue] = pixels.subarray(i, i + 3);
          if (red > 50 && red - green > 21 && red - blue > 15) removal++;
          if (green > 40 && green - red > 20 && green - blue > 8) addition++;
        }
        return keyword >= 4 ? Math.min(removal, addition) : 0;
      }, data);
    }, { timeout: 25_000, message: "G shows both diff colors and syntax-colored keywords on the same wall" }).toBeGreaterThan(70);
    await writeFile(testInfo.outputPath("syntax-diff.png"), screenshot);
  } finally {
    await cdp.detach();
  }
  await expect(page.getByRole("button", { name: "Changed files only" })).toHaveCount(0);
  const center = { x: 640, y: 400 };
  await canvas.click({ position: center, delay: 150 });
  const capture = () => page.evaluate(() => document.pointerLockElement?.id);
  await expect.poll(capture).toBe("repo-city");
  await canvas.evaluate(element => element.blur());
  await expect.poll(capture).toBe("repo-city");
  await page.keyboard.press("KeyF", { delay: 150 });
  await expect(canvas).toHaveAttribute("data-changes-only", "");
  await expect.poll(capture).toBe("repo-city");
  await canvas.focus();
  await page.keyboard.press("Meta+KeyP", { delay: 150 });
  await expectOpen(page, "Find a file");
  await page.keyboard.type("LICENSE", { delay: 55 });
  await page.keyboard.press("Enter", { delay: 150 });
  await expectOpen(page, "Find a file");
  await page.keyboard.press("Escape", { delay: 150 });
  await expectClosed(page);
  await canvas.click({ position: center, delay: 180 });
  await expect.poll(capture).toBe("repo-city");
  await canvas.click({ position: center, button: "right", delay: 180 });
  await expect(canvas).toHaveAttribute("data-action-menu", /^(Fly to face|Find file)/);
  await expect.poll(capture).toBe("repo-city");
  await page.mouse.wheel(0, -120);
  await expect(canvas).toHaveAttribute("data-action-menu", /Show all files/);
  await page.mouse.click(center.x, center.y, { delay: 180 });
  await expect(canvas).not.toHaveAttribute("data-action-menu");
  await expect(canvas).not.toHaveAttribute("data-changes-only", "");
  await expect.poll(capture).toBe("repo-city");
  await canvas.click({ position: center, button: "right", delay: 180 });
  await expect(canvas).toHaveAttribute("data-action-menu", /^(Fly to face|Find file)/);
  await page.mouse.wheel(0, -120);
  await expect(canvas).toHaveAttribute("data-action-menu", /Only changes/);
  await canvas.click({ position: center, button: "right", delay: 180 });
  await expect(canvas).not.toHaveAttribute("data-action-menu");
  await expect.poll(capture).toBe("repo-city");
});


test("selects a closed PR in the picker and permits a signed-in viewer to annotate it", async ({ page }) => {
  const seen = [];
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: true, login: "reviewer" } }));
  await page.route("**/github/api/**", route => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    if (path.endsWith("/pulls")) {
      const state = url.searchParams.get("state");
      seen.push(state);
      if (state === "open") return route.fulfill({ json: [{ number: 1, state: "open", title: "Older open PR" }] });
      return route.fulfill({ json: [
        { number: 7, state: "closed", title: "Retired approach" },
        { number: 8, state: "closed", merged_at: "2026-11-15T18:00:00Z", title: "Shipped approach" },
      ] });
    }
    if (path.endsWith("/pulls/7")) return route.fulfill({ json: {
      state: "closed", base: { sha: before }, head: { sha: after, repo: null },
    } });
    if (path.includes("/compare/")) return route.fulfill({ json: { merge_base_commit: { sha: before } } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [{
      path: "src/history.rs", type: "blob", mode: "100644", sha: path.endsWith(after) ? after : before, size: 16,
    }] } });
    if (path.includes("/git/blobs/")) {
      const source = path.endsWith(after) ? "fn after() {}" : "fn before() {}";
      return route.fulfill({ json: { encoding: "base64", content: Buffer.from(source).toString("base64"), size: source.length } });
    }
    return route.fulfill({ json: path.includes("installations") ? { installations: [] } : [] });
  });
  await page.goto("/?picker=1&repo=owner%2Fdemo");
  await page.getByRole("button", { name: "Pull requests" }).click();
  await expect(page.getByRole("option", { name: /Open · Older open PR/ })).toBeVisible();
  await expect(page.getByRole("option", { name: /Merged · Shipped approach/ })).toBeVisible();
  await page.getByRole("option", { name: /Closed · Retired approach/ }).click();
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  expect(seen.sort()).toEqual(["closed", "open"]);
  const snapshot = await page.evaluate(() => JSON.parse(globalThis.repoCity.snapshot()));
  expect(snapshot).toMatchObject({ name: "owner/demo#7", revision: after, baseline: before });
  expect(await page.evaluate(() => globalThis.repoCity.reviewCanPaint())).toBe(true);
});

test("finds account repositories and revisions in their own fields", async ({ page }, testInfo) => {
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: true, login: "owner" } }));
  await page.route("**/github/api/**", route => {
    const path = new URL(route.request().url()).pathname;
    const values = {
      "/github/api/users/owner/repos": [
        { full_name: "owner/fork", fork: true },
        ...Array.from({ length: 88 }, (_, index) => ({ full_name: `owner/demo-${index}` })),
      ],
      "/github/api/search/commits": { items: [{ sha: next, commit: { message: "Tune the old motor" }, repository: { full_name: "owner/secret" } }] },
      "/github/api/user/starred": [{ full_name: "other/star", fork: false }],
      "/github/api/user/installations": { installations: [{ id: 12 }] },
      "/github/api/user/installations/12/repositories": { repositories: [{ full_name: "owner/secret", private: true }] },
      "/github/api/repos/owner/secret/branches": [{ name: "feature/robot" }],
      "/github/api/repos/owner/secret/tags": [{ name: "v2" }],
      "/github/api/repos/owner/secret/commits": [{ sha: before, commit: { message: "Paint the robot" } }],
    };
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [{ sha: before }] } });
    if (path.includes("/compare/")) return route.fulfill({ json: { merge_base_commit: { sha: before } } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: {
      tree: [{ path: "private.rs", type: "blob", mode: "100644", sha: path.split("/").at(-1), size: 16 }],
    } });
    if (path.includes("/git/blobs/")) return route.fulfill({ json: {
      size: 16, encoding: "base64", content: Buffer.from("fn private() {}").toString("base64"),
    } });
    return route.fulfill({ json: values[path] });
  });
  await page.goto("/");
  const repo = page.getByRole("combobox", { name: "GitHub repository" });
  await expect(page.getByRole("button", { name: /^Revision:/ })).toBeHidden();
  await repo.fill("missing");
  await expect(page.getByText("No matches in your repositories, stars, or forks.")).toBeVisible();
  await repo.fill("demo");
  await expect(page.getByRole("option")).toHaveCount(8);
  await expect(page.getByRole("button", { name: /^Revision:/ })).toBeHidden();
  await page.screenshot({ path: testInfo.outputPath("repository-search.png") });
  await repo.fill("demo-87");
  await expect(page.getByRole("option", { name: "owner/demo-87", exact: false })).toHaveCount(1);
  await repo.fill("other star");
  await expect(page.getByRole("option", { name: "other/star · Star" })).toBeVisible();
  await repo.fill("fork");
  await expect(page.getByRole("option", { name: "owner/fork · Fork" })).toBeVisible();
  await repo.fill("secret");
  await expect(page.getByRole("option", { name: "owner/secret · Private" })).toBeVisible();
  await repo.press("ArrowDown");
  await repo.press("Enter");
  await expect(page.getByText("owner/secret", { exact: true })).toBeVisible();
  await expect(repo).toBeHidden();
  const revision = page.getByRole("button", { name: /^Revision:/ });
  await revision.click();
  const revisionSearch = page.getByRole("combobox", { name: "Find a branch, tag, or commit" });
  await revisionSearch.fill("feature");
  await page.getByRole("option", { name: "Branch · feature/robot" }).click();
  await expect(revision).toHaveAccessibleName("Revision: feature/robot");
  const base = page.getByRole("button", { name: /^Compare with/ });
  await base.click();
  const search = page.waitForRequest(request => {
    const url = new URL(request.url());
    return url.pathname === "/github/api/search/commits" && url.searchParams.get("q").includes("motor");
  });
  await revisionSearch.fill("motor");
  await search;
  await page.getByRole("option", { name: /Tune the old motor/ }).click();
  await expect(base).toHaveAccessibleName("Compare with: eeeeeee");
  await expect(revision).toHaveAccessibleName("Revision: feature/robot");
  await expect(revisionSearch).toBeHidden();
  await page.screenshot({ path: testInfo.outputPath("repository-selected.png") });
  const blob = page.waitForResponse(response => response.url().endsWith("/github/api/repos/owner/secret/git/blobs/" + after));
  await page.getByRole("button", { name: "Open city" }).click();
  expect((await blob).ok()).toBe(true);
  await expect(page.locator("form")).toBeHidden({ timeout: 90_000 });
});

test("restores review progress after reload and resets only the open comparison", async ({ page }, testInfo) => {
  await emulatePointerLock(page);
  let head = after;
  await page.route("https://api.github.com/**", async route => {
    const path = new URL(route.request().url()).pathname;
    const source = Array.from({ length: 30 }, (_, i) => `// row ${i}\n`).join("");
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: head, parents: [{ sha: before }] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: {
      tree: [{ path: "src/demo.rs", type: "blob", mode: "100644", sha: after, size: source.length }],
    } });
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });

  const url = "/?repo=owner%2Fdemo&ref=main";
  const ready = () => expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.goto(url);
  await ready();
  const canvas = page.locator("canvas");
  const summary = page.locator("#progress-summary");
  const polling = { timeout: 25_000, intervals: [250, 500, 1000] };
  await canvas.focus();
  await page.keyboard.press("KeyV", { delay: 150 });
  const read = scope => page.evaluate(async scope => {
    const { readProgress } = await import("/progress.mjs");
    return readProgress(scope);
  }, scope);
  const scope = JSON.stringify(["owner/demo", after, before]);
  const separate = JSON.stringify(["owner/other", after, before]);
  const reviewed = JSON.stringify({ reviewed: "src/demo.rs" });
  await expect.poll(() => read(scope), polling).toContain(reviewed);
  const visited = JSON.stringify({ visited: { path: "src/demo.rs", diff: false } });
  await expect.poll(() => read(scope), polling).toContain(visited);
  await canvas.evaluate(element => {
    element.addEventListener("pointerdown", () => {
      // Simulate free mouse motion queued just before the click acquires lock.
      const motion = new PointerEvent("pointermove", { pointerType: "mouse", button: -1, movementY: 650 });
      Object.defineProperty(motion, "getCoalescedEvents", { value: () => [motion] });
      window.dispatchEvent(motion);
    }, { capture: true, once: true });
  });
  await canvas.click({ position: { x: 640, y: 400 }, delay: 150 });
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect((await read(scope)).some(mark => JSON.parse(mark).reveal)).toBe(false);
  await canvas.click({ position: { x: 640, y: 400 }, button: "right" });
  await expect(canvas).toHaveAttribute("data-action-menu");
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
  await canvas.screenshot({ path: testInfo.outputPath("city-actions.png") });
  await page.keyboard.press("Escape", { delay: 120 });
  await expect(canvas).not.toHaveAttribute("data-action-menu");
  await expect.poll(() => page.evaluate(() => document.pointerLockElement)).toBeNull();
  await canvas.click({ position: { x: 640, y: 400 }, button: "right" });
  await expect(canvas).toHaveAttribute("data-action-menu");
  await page.keyboard.press("Meta+KeyP", { delay: 150 });
  await expectOpen(page, "Find a file");
  await page.keyboard.type("demo.rs", { delay: 45 });
  await expectOpen(page, "Find a file");
  await canvas.screenshot({ path: testInfo.outputPath("city-file-search.png") });
  await page.keyboard.press("Escape", { delay: 120 });
  await expectClosed(page);
  await page.mouse.move(640, 400);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await canvas.click({ position: { x: 640, y: 400 }, delay: 150 });
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.mouse.click(640, 400, { delay: 100 });
  await page.waitForTimeout(400);
  await page.mouse.click(640, 400, { delay: 100 });
  expect((await read(scope)).some(mark => JSON.parse(mark).reveal)).toBe(false);
  await page.mouse.click(640, 400, { delay: 100 });
  await expect.poll(async () => (await read(scope)).some(mark => JSON.parse(mark).reveal), polling).toBe(true);
  await page.mouse.click(640, 400, { delay: 100 });
  await canvas.screenshot({ path: testInfo.outputPath("city-window-washer.png") });
  const marks = await read(scope);
  await page.keyboard.press("Escape", { delay: 120 });
  await page.evaluate(async ({ separate, reviewed }) => {
    const { writeProgress } = await import("/progress.mjs");
    await writeProgress(separate, [[reviewed, true]]);
  }, { separate, reviewed });

  await page.reload();
  await ready();
  await expect(summary).toHaveText(/1 file reviewed · [1-9][0-9]* lines revealed/, polling);
  // Toggling the file after reload removes the restored mark.
  await canvas.focus();
  await page.keyboard.press("KeyV", { delay: 150 });
  await expect.poll(() => read(scope), polling).toEqual(marks.filter(mark => mark !== reviewed));
  await page.keyboard.press("KeyV", { delay: 150 });
  await expect.poll(() => read(scope), polling).toContain(reviewed);

  await page.getByRole("button", { name: "Repo City", exact: true }).click();
  await expect(summary).toHaveText(/1 file reviewed · [1-9][0-9]* lines revealed/);
  await page.getByText("Progress", { exact: true }).click();
  const reset = page.getByRole("button", { name: "Reset progress" });
  page.once("dialog", prompt => prompt.dismiss());
  await reset.click();
  await expect(await read(scope)).toContain(reviewed);
  page.once("dialog", prompt => prompt.accept());
  await reset.click();
  await expect(page.getByRole("status")).toHaveText("Progress reset for this comparison.");
  await expect(await read(scope)).toEqual([]);
  await expect(summary).toHaveText("0 files reviewed · 0 lines revealed");
  await expect(await read(separate)).toContain(reviewed);
  await page.getByRole("button", { name: "Close repository picker" }).click();
  await canvas.focus();
  await page.keyboard.press("KeyV", { delay: 150 });
  await expect.poll(() => read(scope), polling).toContain(reviewed);

  await page.keyboard.press("Slash", { delay: 150 });
  await expect(canvas).toHaveAttribute("data-pointer-ui", "");
  await page.keyboard.type("row 2", { delay: 40 });
  await canvas.screenshot({ path: testInfo.outputPath("city-find-text.png") });
  await page.keyboard.press("Enter", { delay: 150 });
  await expect(canvas).not.toHaveAttribute("data-pointer-ui", "");
  await page.keyboard.press("KeyN", { delay: 150 });
  await page.keyboard.press("Shift+KeyN", { delay: 150 });
  await canvas.evaluate(element => element.requestPointerLock());
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
  await page.keyboard.press("KeyM", { delay: 150 });
  // Both overview stages keep the cursor free for map navigation.
  await page.keyboard.press("KeyD", { delay: 1_500 });
  await expect(canvas).toHaveAttribute("data-overview", "");
  await expect.poll(() => page.evaluate(() => document.pointerLockElement)).toBeNull();
  await page.keyboard.press("KeyM", { delay: 150 });
  await expect(canvas).toHaveAttribute("data-overview", "");
  await page.keyboard.press("KeyM", { delay: 150 });
  await expect(canvas).not.toHaveAttribute("data-overview", "");
  head = next;
  await page.reload();
  await ready();
  await expect.poll(() => read(JSON.stringify(["owner/demo", next, before])), polling).toEqual([visited]);
});

test("publishes a painted preset in one action and still supports suggestions", async ({ page }, testInfo) => {
  // This flow drags over a multi-line range; make its readable block explicit.
  await page.addInitScript(() => localStorage.setItem("repo-city-reading-v1", JSON.stringify({
    mode: "delayed", block_kind: "lines", auto_advance: false, block_lines: 16, min_seconds: 3, max_seconds: 6,
  })));
  await emulatePointerLock(page);
  let remoteMark;
  let viewed = false;
  let reactionAttempts = 0;
  const installUrl = "https://github.com/apps/repo-city-viewer/installations/new";
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: true, login: "reviewer", installUrl } }));
  await page.route(/\/github\/api\/|https:\/\/api.github.com\//, async route => {
    const path = new URL(route.request().url()).pathname.replace("/github/api", "");
    const source = Array.from({ length: 32 }, (_, i) =>
      `fn inspect_review_case_${i}() { /* ${i === 3 && path.endsWith(before) ? "before" : "after"} */ }`,
    ).join("\n") + "\n";
    if (path === "/repos/owner/demo/pulls/8") return route.fulfill({ json: {
      state: "open", base: { sha: before }, head: { sha: after },
    } });
    if (path.endsWith("/comments")) return route.fulfill({ json: [
      { id: 13, node_id: "comment-13", path: "src/demo.rs", line: 25, side: "RIGHT", body: "**Separate note**", user: { login: "ada" } },
      { id: 14, node_id: "comment-14", in_reply_to_id: 13, body: "Got it, @Reviewer. Check the line above.", user: { login: "sam" } },
      { id: 15, node_id: "comment-15", path: "src/demo.rs", line: 26, side: "RIGHT", body: "Resolved note", user: { login: "ada" } },
      ...(remoteMark ? [remoteMark] : []),
    ] });
    if (path.includes("/compare/")) return route.fulfill({ json: { merge_base_commit: { sha: before } } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [{
      path: "src/demo.rs", type: "blob", mode: "100644", sha: path.endsWith(after) ? after : before, size: source.length,
    }] } });
    if (path.includes("/git/blobs/")) return route.fulfill({ json: {
      size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64"),
    } });
    return route.fulfill({ json: path.includes("installations") ? { installations: [] } : [] });
  });
  const posts = [];
  await page.route("**/github/review/owner/demo/8", route => {
    if (route.request().method() === "GET") return route.fulfill({ json: {
      headRefOid: after,
      reviewThreads: { pageInfo: { hasNextPage: false }, nodes: [
        { isResolved: false, comments: { nodes: [{ id: "comment-13" },
          { id: "comment-14", reactionGroups: [{ content: "EYES", viewerHasReacted: viewed, users: { totalCount: 1 } }] },
        ] } },
        { isResolved: true, comments: { nodes: [{ id: "comment-15" }] } },
      ] },
    } });
    const body = route.request().postDataJSON();
    posts.push(body);
    if (posts.length === 1) return route.fulfill({ status: 403, json: { error: "Publishing needs Pull requests: write." } });
    const html_url = "https://github.com/owner/demo/pull/8#discussion_r" + (40 + posts.length);
    if (posts.length === 2) remoteMark = {
      id: 42, node_id: "comment-42", path: body.path, line: body.end, start_line: body.start, side: "RIGHT",
      body: `${body.text}

<!-- repo-city:v1:${body.id}:${body.preset} -->`,
      html_url, user: { login: "reviewer" },
    };
    return route.fulfill({ status: 201, json: { comment: { html_url } } });
  });
  await page.route("**/github/review/owner/demo/8/comments/14/eyes", route => {
    expect(route.request().method()).toBe("POST");
    if (++reactionAttempts === 1) return route.fulfill({ status: 403, json: { error: "GitHub requires Pull requests: write." } });
    viewed = true;
    return route.fulfill({ json: { viewed: true } });
  });
  const url = "/?pr=https%3A%2F%2Fgithub.com%2Fowner%2Fdemo%2Fpull%2F8";
  await page.goto(url);
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas");
  const center = { x: 640, y: 400 };
  let approached = false;
  const lock = async () => {
    await page.mouse.move(center.x, center.y);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await canvas.evaluate(element => element.requestPointerLock());
    await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    if (!approached) {
      await page.keyboard.press("KeyE", { delay: 180 });
      await page.waitForTimeout(1800);
      approached = true;
    }
  };
  const review = page.locator("#review-dialog");
  await lock();
  expect(await page.evaluate(() => globalThis.repoCity.reviewCanPaint())).toBe(true);
  await page.mouse.click(640, 400, { delay: 100 });
  await page.waitForTimeout(300);
  await canvas.screenshot({ path: testInfo.outputPath("water-countdown-ring.png") });
  await expect(page.locator("#progress-summary")).toHaveText(/[1-9]\d* lines revealed/, { timeout: 65_000 });
  await page.mouse.click(640, 400, { delay: 100 });
  await page.keyboard.press("KeyU", { delay: 120 });
  await expectOpen(page);
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Edit paint slot 8" }));
  await gpuFill(page, review.getByLabel("Name"), "Investigate");
  await gpuFill(page, review.getByLabel("Spray text"), "Check this path");
  await gpuFill(page, review.getByLabel("Color"), "#ff3b86");
  await gpuFill(page, review.getByLabel("Reusable prompt"), "Reproduce first.");
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Save paint" }));
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "8 · Investigate" }));
  await lock();
  await page.mouse.down();
  await page.waitForTimeout(200);
  await page.mouse.wheel(0, 38);
  await page.waitForTimeout(260);
  await canvas.screenshot({ path: testInfo.outputPath("live-paint.png") });
  await page.mouse.up();
  await expect.poll(() => page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes.filter(note => !note.remote).length)).toBe(1);
  await expectClosed(page);
  await page.keyboard.press("KeyJ", { delay: 120 });
  await expectOpen(page);
  const draft = review.getByRole("button", { includeHidden: true, name: /Investigate.*Draft/ });
  if (await draft.count()) await gpuClick(page, draft);
  await expect(review.locator(".markdown-preview").getByText("Reproduce first.", { exact: true })).toBeAttached();
  await expect(review.getByLabel("Comment")).toBeHidden();
  expect(posts).toHaveLength(0);
  const firstMark = await page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes.find(note => !note.remote));
  expect(firstMark.anchor.end).toBeGreaterThan(firstMark.anchor.start);
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Publish to GitHub" }));
  await expect(review.getByRole("status", { includeHidden: true })).toContainText("Pull requests: write");
  expect(posts[0]).toMatchObject({ ...firstMark.anchor, revision: after, preset: firstMark.preset.id, text: "Reproduce first." });
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Publish to GitHub" }));
  await expect(review.getByRole("link", { includeHidden: true, name: "Published on GitHub" })).toBeAttached();
  await page.keyboard.press("Tab", { delay: 120 });
  await expectClosed(page);
  await lock();
  await page.mouse.down();
  await page.waitForTimeout(350);
  await page.mouse.up();
  const unpublished = () => page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes.filter(note => !note.remote && !note.published).length);
  expect(await unpublished()).toBe(0);
  await page.keyboard.press("Digit2", { delay: 120 });
  await page.mouse.down();
  await page.waitForTimeout(350);
  await page.mouse.up();
  await expect.poll(unpublished).toBeGreaterThan(0);
  await expectClosed(page);
  await page.keyboard.press("KeyJ", { delay: 120 });
  await expectOpen(page);
  const suggestion = review.getByRole("button", { includeHidden: true, name: /Tests.*Draft/ });
  if (await suggestion.count()) await gpuClick(page, suggestion);
  await expect(review.getByRole("button", { includeHidden: true, name: "Edit comment" })).toBeAttached({ timeout: 20_000 });
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Write a comment" }));
  await expect(review.getByLabel("Comment")).toHaveValue("");
  await gpuFill(page, review.getByLabel("Comment"), "Check this specific behavior.");
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Suggest replacement" }));
  await gpuFill(page, review.getByLabel("Replacement code"), "    // proposed replacement");
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Preview GitHub suggestion" }));
  await expect(review.getByText("Check this specific behavior.", { exact: false }).last()).toBeAttached();
  await expect(review.getByText("    // proposed replacement", { exact: false })).toBeAttached();
  await expect(review.getByText("Publishing requires write access to pull requests.")).toBeAttached();
  await expect(review.getByRole("link", { includeHidden: true, name: "Manage repository access" })).toHaveAttribute("href", installUrl);
  expect(posts).toHaveLength(2);
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Publish to GitHub" }));
  await expect(review.getByRole("link", { includeHidden: true, name: "Published on GitHub" })).toBeAttached();
  expect(posts[2]).toMatchObject({ path: "src/demo.rs", revision: after, before: false, text: "Check this specific behavior.", replacement: "    // proposed replacement" });
  await page.reload();
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect.poll(() => page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes
    .find(note => note.id === "github-42")?.preset.color), { timeout: 20_000 }).toEqual([255, 59, 134]);
  const restored = await page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()));
  expect(restored.palette[7].label).toBe("Investigate");
  expect(restored.notes.find(note => note.id === "github-42")).toMatchObject({
    anchor: firstMark.anchor, text: "Reproduce first.", remote: true,
  });
  await canvas.screenshot({ path: testInfo.outputPath("restored-paint-close.png") });
  await canvas.focus();
  await page.keyboard.press("Home", { delay: 120 });
  await canvas.screenshot({ path: testInfo.outputPath("restored-paint-medium.png") });
  await page.keyboard.press("Home", { delay: 120 });
  expect(restored.notes.find(note => !note.remote && note.replacement)).toMatchObject({
    published: "https://github.com/owner/demo/pull/8#discussion_r43",
    text: "Check this specific behavior.",
    replacement: "    // proposed replacement",
  });
  await lock();
  await page.mouse.click(center.x, center.y, { button: "right" });
  await expect(canvas).toHaveAttribute("data-action-menu");
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
  await page.keyboard.press("KeyJ", { delay: 120 });
  const thread = review.getByRole("button", { includeHidden: true, name: /Comment.*demo\.rs:25.*For you/ });
  await expect(thread).toBeAttached();
  const resolved = review.getByRole("button", { includeHidden: true, name: /demo\.rs:26.*Resolved/ });
  await expect(resolved).toHaveCount(0);
  await gpuClick(page, review.getByRole("checkbox", { includeHidden: true, name: "Show resolved threads" }));
  await expect(resolved).toBeAttached();
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Fly to next mention" }));
  await expectClosed(page);
  await lock();
  await page.keyboard.press("KeyJ", { delay: 120 });
  await expect(review.getByRole("heading", { includeHidden: true, name: "Review · PR #8" })).toBeHidden();
  await expect.poll(async () => {
    const title = await review.evaluate(dialog => dialog.open ? dialog.querySelector("h2").textContent : null);
    if (title === "src/demo.rs:25") return true;
    if (title !== null) {
      await page.keyboard.press("Tab", { delay: 120 });
      await expectClosed(page);
      return false;
    }
    if (!await page.evaluate(() => Boolean(document.pointerLockElement))) await lock();
    await page.keyboard.press("KeyJ", { delay: 120 });
    return false;
  }, { timeout: 20_000 }).toBe(true);
  await expect(review.locator(".markdown-preview").getByText("Separate note", { exact: false })).toBeAttached();
  await expect(review.getByText("Got it, @Reviewer. Check the line above.")).toBeAttached();
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Mark as viewed" }));
  await expect(review.locator("article").getByRole("status", { includeHidden: true })).toContainText("Pull requests: write");
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Mark as viewed" }));
  await expect(review.getByText("Viewed on GitHub")).toBeAttached();
  await gpuSelect(page, review.getByLabel("Local paint"), { label: "Investigate" });
  await gpuClick(page, review.getByRole("button", { includeHidden: true, name: "Save local paint" }));
  await expect(review.getByRole("button", { includeHidden: true, name: /Investigate.*demo\.rs:25.*GitHub thread/ })).toBeAttached();
  expect(posts).toHaveLength(3);
  await page.reload();
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect.poll(() => page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes
    .filter(note => note.remote).map(note => [note.id, note.preset.label, note.resolved])), { timeout: 20_000 }).toEqual([
    ["github-13", "Investigate", false], ["github-15", "Comment", true], ["github-42", "Investigate", null],
  ]);
  expect(await page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes
    .find(note => note.id === "github-13").attention)).toBe(false);
  expect(reactionAttempts).toBe(2);
});

test("VibePad owns file search, extensionless license previews, help toggle and exact commit comparison", async ({ page }) => {
  await emulatePointerLock(page);
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  const source = "literal <script>not HTML</script> & text\nsecond line\n";
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const url = new URL(route.request().url()), path = url.pathname;
    if (path === "/repos/owner/tools") return route.fulfill({ json: { default_branch: "main" } });
    if (path === "/repos/owner/tools/commits") return route.fulfill({ json: [
      { sha: after, commit: { message: "New commit", author: { name: "Writer", date: "2026-11-18T12:00:00Z" } } },
      { sha: before, commit: { message: "Old commit", author: { name: "Writer", date: "2026-11-17T12:00:00Z" } } },
    ] });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: path.endsWith(before) ? before : after, parents: [], commit: { message: "Commit detail", author: { name: "Writer" } }, files: [{ filename: "notes.txt", patch: "@@ -1 +1 @@\n-old\n+new" }] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [{ path: "skills/differential-review/UPSTREAM-LICENSE", type: "blob", mode: "100644", sha: after, size: source.length }] } });
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Ftools");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas"); await canvas.focus();
  await expect(canvas).toBeFocused();
  await expect(canvas).not.toHaveAttribute("data-pointer-ui", "");
  await page.keyboard.press("h", { delay: 120 }); await expectOpen(page, "Help");
  await page.keyboard.press("h", { delay: 120 }); await expectClosed(page);
  await page.keyboard.press("Meta+p", { delay: 120 });
  const search = page.locator("#review-dialog"); await expectOpen(page);
  await gpuFill(page, search.getByRole("searchbox", { includeHidden: true }), "missing"); await expect(search.getByRole("status", { includeHidden: true })).toHaveText("No matching files.");
  await gpuFill(page, search.getByRole("searchbox", { includeHidden: true }), "upstream-license"); await page.keyboard.press("ArrowDown", { delay: 120 });
  await gpuFocus(page, search.getByRole("button", { includeHidden: true, name: "skills/differential-review/UPSTREAM-LICENSE", exact: true })); await page.keyboard.press("Enter", { delay: 120 });
  await expectClosed(page);
  await expect(canvas).toBeFocused();
  await page.waitForTimeout(1200);
  await canvas.evaluate(element => element.requestPointerLock()); await page.keyboard.press("KeyP", { delay: 180 });
  const preview = page.locator("#review-dialog"); await expect(preview.locator("pre")).toHaveText(source);
  await expect(preview.locator("script")).toHaveCount(0);
  await page.keyboard.press("Tab", { delay: 120 }); await expectClosed(page);
  await page.keyboard.press("Tab", { delay: 120 }); await page.keyboard.press("Alt+3", { delay: 120 });
  const log = page.locator("#review-dialog");
  await expect(log.getByText("New commit", { exact: false })).toBeAttached();
  await gpuClick(page, log.locator("article").filter({ hasText: "Old commit" }).getByRole("button", { includeHidden: true, name: "Before", exact: true }));
  await gpuClick(page, log.locator("article").filter({ hasText: "New commit" }).getByRole("button", { includeHidden: true, name: "After", exact: true }));
  await gpuClick(page, log.locator("article").filter({ hasText: "New commit" }).getByRole("button", { includeHidden: true }).first());
  await expect(log.locator("pre")).toContainText("+new");
  await gpuClick(page, log.getByRole("button", { includeHidden: true, name: "Compare selected commits" }));
  await expectClosed(page); await expect(page).toHaveURL(/exact=1/);
  expect(new URL(page.url()).searchParams.get("base")).toBe(before);
});

test("mobile Pad button opens and puts away the GPU tablet", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 844, height: 390 }, isMobile: true, hasTouch: true });
  const page = await context.newPage();
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/mobile") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [{ path: "main.rs", type: "blob", mode: "100644", sha: after, size: 13 }] } });
    return route.fulfill({ json: { encoding: "base64", content: Buffer.from("fn main() {}\n").toString("base64"), size: 13 } });
  });
  await page.goto("/?repo=owner%2Fmobile");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.getByRole("button", { name: "Open VibePad", exact: true }).tap();
  const pad = page.locator("#review-dialog"); await expectOpen(page);
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Put away VibePad" }), true);
  await expectClosed(page); await expect(page.getByRole("button", { name: "Open VibePad", exact: true })).toBeVisible();
  await context.close();
});

test("P previews images and keeps corrupt images and binary files inside VibePad", async ({ page }) => {
  await emulatePointerLock(page);
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=", "base64");
  const binary = "c".repeat(40);
  const images = { [after]: png, [before]: Buffer.from("broken PNG"), [binary]: Buffer.from([65, 0, 66]) };
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/images") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [
      { path: "image.png", type: "blob", mode: "100644", sha: after, size: png.length },
      { path: "broken.png", type: "blob", mode: "100644", sha: before, size: 10 },
      { path: "binary.unknown", type: "blob", mode: "100644", sha: binary, size: 3 },
    ] } });
    const bytes = images[path.split("/").pop()];
    return route.fulfill({ json: { size: bytes.length, encoding: "base64", content: bytes.toString("base64") } });
  });
  await page.goto("/?repo=owner%2Fimages");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  for (const name of ["image.png", "broken.png", "binary.unknown"]) {
    await page.keyboard.press("Meta+p", { delay: 120 });
    const search = page.locator("#review-dialog");
    await gpuClick(page, search.getByRole("button", { includeHidden: true, name, exact: true }));
    await expectClosed(page);
    await page.waitForTimeout(1200);
    await page.locator("canvas").evaluate(element => element.requestPointerLock());
    await page.keyboard.press("KeyP", { delay: 180 });
    const preview = page.locator("#review-dialog");
    if (name === "image.png") {
      await expect(preview.getByRole("img", { includeHidden: true, name })).toBeAttached();
      // The hidden service image carries raw bytes; GPU pixel rendering is covered by image-codecs.spec.
      await expect(preview.getByRole("img", { includeHidden: true })).toHaveAttribute("data-preview-source", /^blob:/);
    } else await expect(preview).toContainText(name === "broken.png" ? "Image data is invalid or unsupported." : "Binary asset: metadata only");
    await page.keyboard.press("Tab", { delay: 120 }); await expectClosed(page);
  }
});

test("P plays a video from the selected snapshot and explains unsupported video data", async ({ page }) => {
  await emulatePointerLock(page);
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  const clip = await readFile(new URL("./fixtures/video-preview.webm", import.meta.url));
  const invalid = Buffer.from("invalid video");
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/video") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [
      { path: "clip.webm", type: "blob", mode: "100644", sha: after, size: clip.length },
      { path: "invalid.webm", type: "blob", mode: "100644", sha: before, size: invalid.length },
    ] } });
    const bytes = path.endsWith(before) ? invalid : clip;
    return route.fulfill({ json: { size: bytes.length, encoding: "base64", content: bytes.toString("base64") } });
  });
  await page.goto("/?repo=owner%2Fvideo");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  for (const name of ["clip.webm", "invalid.webm"]) {
    await page.keyboard.press("Meta+p", { delay: 120 });
    const preview = page.locator("#review-dialog");
    await gpuClick(page, preview.getByRole("button", { includeHidden: true, name, exact: true }));
    await expectClosed(page);
    await page.waitForTimeout(1200);
    await page.locator("canvas").evaluate(element => element.requestPointerLock());
    await page.keyboard.press("KeyP", { delay: 180 });
    await expect(preview).toBeVisible();
    if (name === "clip.webm") {
      const video = preview.locator("video");
      await expect(video).toHaveAttribute("controls", "");
      await video.evaluate(element => element.play());
      await expect.poll(() => video.evaluate(element => element.currentTime)).toBeGreaterThan(0);
    } else {
      await expect(preview).toContainText("This video could not be played.");
    }
    await preview.getByRole("button", { name: "Close", exact: true }).click();
    await expect(preview).toBeHidden();
  }
});

test("keyboard-only navigation can find a building and wash source without pointer capture", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("repo-city-guide-v1", "seen");
    localStorage.setItem("repo-city-reading-v1", JSON.stringify({ mode: "immediate", block_lines: 6, block_kind: "lines", auto_advance: true, min_seconds: null, max_seconds: null }));
  });
  const source = "fn main() {\n    let answer = 42;\n    println!(\"{answer}\");\n}\n";
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/keyboard") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [{ path: "main.rs", type: "blob", mode: "100644", sha: after, size: source.length }] } });
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Fkeyboard");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.keyboard.press("Meta+p", { delay: 120 });
  const find = page.locator("#review-dialog"); await expectOpen(page);
  await page.keyboard.type("main", { delay: 40 });
  await expect(find.getByRole("searchbox", { includeHidden: true })).toHaveValue("main");
  await expectOpen(page, "Find a file");
  await page.keyboard.press("ArrowDown", { delay: 120 });
  await gpuFocus(page, find.getByRole("button", { includeHidden: true, name: "main.rs", exact: true }));
  await page.keyboard.press("Enter", { delay: 120 }); await expectClosed(page);
  await page.waitForTimeout(1600);
  await page.keyboard.press("KeyE", { delay: 180 });
  await page.waitForTimeout(2500);
  await page.keyboard.press("KeyK", { delay: 180 });
  await expect.poll(async () => Number((await page.locator("#progress-summary").textContent()).match(/(\d+) lines revealed/)?.[1] ?? 0), { timeout: 15_000 }).toBeGreaterThan(0);
  expect(await page.evaluate(() => document.pointerLockElement)).toBeNull();
  await page.keyboard.press("Tab", { delay: 120 }); await expectOpen(page, "Changed files");
});
