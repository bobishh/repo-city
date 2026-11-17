import { test, expect } from "@playwright/test";
import { writeFile } from "node:fs/promises";

const after = "b".repeat(40);
const before = "a".repeat(40);
const next = "e".repeat(40);

test("shows the guide once and keeps it available from the help button", async ({ page }) => {
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
  await expect(page.getByRole("link", { name: "Build for Mac" })).toHaveAttribute("href", "https://github.com/bobishh/repo-city#on-your-mac");
  const guide = page.getByRole("dialog", { name: "Take a quick tour" });
  const help = page.getByRole("button", { name: "How to use Repo City" });
  await expect(guide).toBeHidden();
  await expect(help).toBeHidden();
  await page.getByRole("combobox", { name: "GitHub repository" }).fill("owner/demo");
  await page.getByRole("button", { name: "Select repository" }).click();
  await page.getByRole("button", { name: "Open city" }).click();
  try {
    await expect(page.locator("#status")).toContainText("Loading repository");
    await expect(guide).toBeHidden();
    await expect(help).toBeHidden();
  } finally {
    releaseTree();
  }
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect(page.locator("canvas")).toBeVisible();
  expect(await page.evaluate(() => globalThis.repoCity.reviewCanPaint())).toBe(false);
  await expect(guide).toBeVisible();
  await expect(guide.getByText("Click the Repo City logo to open the repository picker.")).toBeVisible();
  await guide.getByRole("button", { name: "Skip", exact: true }).click();
  await expect(guide).toBeHidden();
  await page.reload();
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect(guide).toBeHidden();
  await help.click();
  await expect(guide).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(guide).toBeHidden();
  await page.evaluate(() => localStorage.removeItem("repo-city-guide-v1"));
  await page.reload();
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await guide.getByRole("button", { name: "Start tour" }).click();
  await expect(guide).toBeHidden();
  expect(await page.evaluate(() => globalThis.repoCity.tourCommand() & 1)).toBe(1);
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
  await expect(page.getByRole("button", { name: "Repo City", exact: true })).toBeVisible();
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
    const portrait = await page.evaluate(async data => {
      const image = await createImageBitmap(new Blob([Uint8Array.fromBase64(data)], { type: "image/png" }));
      const head = new OffscreenCanvas(48, 48).getContext("2d");
      head.drawImage(image, 16, 16, 48, 48, 0, 0, 48, 48);
      image.close();
      const { data: pixels } = head.getImageData(0, 0, 48, 48);
      let metal = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        const [red, green, blue] = pixels.subarray(i, i + 3);
        if (red > 110 && red > green + 12 && green > blue + 7) metal++;
      }
      return metal;
    }, png.toString("base64"));
    expect(portrait, "WebGPU renders the android in the logo").toBeGreaterThan(30);
    await writeFile(testInfo.outputPath("city.png"), png);
  } catch (error) {
    if (png) await writeFile(testInfo.outputPath("last-canvas.png"), png);
    await writeFile(testInfo.outputPath("browser-console.txt"), consoleLog.join("\n"));
    throw error;
  } finally {
    await cdp.detach();
  }
  expect(faults).toEqual([]);
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
  const committed = page.waitForRequest(request => request.url().endsWith("/commits/" + next));
  const replacement = page.waitForResponse(response => response.url().endsWith("/git/blobs/" + next));
  await dialog.getByRole("button", { name: "Open city" }).click();
  await committed;
  expect((await replacement).ok()).toBe(true);
  await expect(dialog).toBeHidden();
  await expect(page).toHaveURL(new RegExp("ref=" + next));
  await page.getByRole("button", { name: "Repo City", exact: true }).click();
  await page.keyboard.press("Escape");
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
  await page.keyboard.press("Escape");
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
  await expect(page.getByRole("button", { name: "Repo City", exact: true })).toBeVisible();
  await expect(page.getByRole("dialog", { name: "Take a quick tour" })).toBeHidden();
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
  await expect(page.locator("#progress-warning")).toContainText("Another Repo City tab is blocking the progress update.");
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
  await page.keyboard.press("KeyP", { delay: 150 });
  await expect(canvas).toHaveAttribute("data-pointer-ui", "");
  await page.keyboard.type("LICENSE", { delay: 55 });
  await page.keyboard.press("Enter", { delay: 150 });
  await expect(canvas).toHaveAttribute("data-pointer-ui", "");
  await page.keyboard.press("Escape", { delay: 150 });
  await expect(canvas).not.toHaveAttribute("data-pointer-ui", "");
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


test("selects a closed PR in the picker while a signed-in viewer stays read-only", async ({ page }) => {
  const seen = [];
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: true, login: "reviewer" } }));
  await page.route("**/github/api/**", route => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    if (path.endsWith("/pulls")) {
      seen.push(url.searchParams.get("state"));
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
  await expect(page.getByRole("option", { name: /Merged · Shipped approach/ })).toBeVisible();
  await page.getByRole("option", { name: /Closed · Retired approach/ }).click();
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  expect(seen).toEqual(["all"]);
  const snapshot = await page.evaluate(() => JSON.parse(globalThis.repoCity.snapshot()));
  expect(snapshot).toMatchObject({ name: "owner/demo#7", revision: after, baseline: before });
  expect(await page.evaluate(() => globalThis.repoCity.reviewCanPaint())).toBe(false);
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
  await page.keyboard.press("Escape");
  await expect(canvas).not.toHaveAttribute("data-action-menu");
  await expect.poll(() => page.evaluate(() => document.pointerLockElement)).toBeNull();
  await canvas.click({ position: { x: 640, y: 400 }, button: "right" });
  await expect(canvas).toHaveAttribute("data-action-menu");
  await page.keyboard.press("KeyP", { delay: 150 });
  await expect(canvas).toHaveAttribute("data-pointer-ui", "");
  await page.keyboard.type("demo.rs", { delay: 45 });
  await expect(canvas).toHaveAttribute("data-pointer-ui", "");
  await canvas.screenshot({ path: testInfo.outputPath("city-file-search.png") });
  await page.keyboard.press("Escape");
  await expect(canvas).not.toHaveAttribute("data-pointer-ui", "");
  await canvas.click({ position: { x: 640, y: 400 }, delay: 150 });
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.mouse.down();
  try {
    await expect.poll(async () => (await read(scope)).some(mark => JSON.parse(mark).reveal), polling).toBe(true);
    await canvas.screenshot({ path: testInfo.outputPath("city-window-washer.png") });
  } finally {
    await page.mouse.up();
  }
  const marks = await read(scope);
  await page.keyboard.press("Escape");
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
  // Fly after the overview hop, with the same pointer lock as before it.
  await page.keyboard.press("KeyD", { delay: 1_500 });
  await expect(canvas).not.toHaveAttribute("data-pointer-ui", "");
  await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
  await page.keyboard.press("KeyM", { delay: 150 });
  await expect(canvas).not.toHaveAttribute("data-pointer-ui", "");
  head = next;
  await page.reload();
  await ready();
  await expect.poll(() => read(JSON.stringify(["owner/demo", next, before])), polling).toEqual([visited]);
});

test("publishes a painted preset in one action and still supports suggestions", async ({ page }, testInfo) => {
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
      { id: 13, node_id: "comment-13", path: "src/demo.rs", line: 25, side: "RIGHT", body: "Separate note", user: { login: "ada" } },
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
  const lock = async () => {
    await canvas.click({ position: center, delay: 150 });
    await expect.poll(() => page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  };
  const review = page.locator("#review-dialog");
  await lock();
  expect(await page.evaluate(() => globalThis.repoCity.reviewCanPaint())).toBe(true);
  await page.mouse.down();
  await expect(page.locator("#progress-summary")).toHaveText(/[1-9]\d* lines revealed/, { timeout: 40_000 });
  await page.mouse.up();
  await page.keyboard.press("KeyU", { delay: 120 });
  await expect(review).toBeVisible({ timeout: 20_000 });
  await review.getByRole("button", { name: "Edit paint slot 8" }).click();
  await review.getByLabel("Name").fill("Investigate");
  await review.getByLabel("Spray text").fill("Check this path");
  await review.getByLabel("Color").fill("#ff3b86");
  await review.getByLabel("Reusable prompt").fill("Reproduce first.");
  await review.getByRole("button", { name: "Save paint" }).click();
  await review.getByRole("button", { name: "8 · Investigate" }).click();
  await lock();
  await page.mouse.down();
  await page.waitForTimeout(200);
  await page.mouse.wheel(0, 38);
  await page.waitForTimeout(260);
  await canvas.screenshot({ path: testInfo.outputPath("live-paint.png") });
  await page.mouse.up();
  await expect.poll(() => page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes.filter(note => !note.remote).length)).toBe(1);
  await expect(review).toBeHidden();
  await page.keyboard.press("KeyJ", { delay: 120 });
  await expect(review).toBeVisible({ timeout: 20_000 });
  const draft = review.getByRole("button", { name: /Investigate.*Draft/ });
  if (await draft.isVisible()) await draft.click();
  await expect(review.locator("pre").getByText("Reproduce first.", { exact: true })).toBeVisible();
  await expect(review.getByLabel("Comment")).toBeHidden();
  expect(posts).toHaveLength(0);
  const firstMark = await page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes.find(note => !note.remote));
  expect(firstMark.anchor.end).toBeGreaterThan(firstMark.anchor.start);
  await review.getByRole("button", { name: "Publish to GitHub" }).click();
  await expect(review.getByRole("status")).toContainText("Pull requests: write");
  expect(posts[0]).toMatchObject({ ...firstMark.anchor, revision: after, preset: firstMark.preset.id, text: "Reproduce first." });
  await review.getByRole("button", { name: "Publish to GitHub" }).click();
  await expect(review.getByRole("link", { name: "Published on GitHub" })).toBeVisible();
  await review.getByRole("button", { name: "Close" }).click();
  await lock();
  await page.mouse.down();
  await page.waitForTimeout(350);
  await page.mouse.up();
  await expect.poll(() => page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes.filter(note => !note.remote && !note.published).length)).toBeGreaterThan(0);
  await expect(review).toBeHidden();
  await page.keyboard.press("KeyJ", { delay: 120 });
  await expect(review).toBeVisible({ timeout: 20_000 });
  if (await draft.isVisible()) await draft.click();
  await expect(review.getByRole("button", { name: "Edit comment" })).toBeVisible({ timeout: 20_000 });
  await review.getByRole("button", { name: "Write a comment" }).click();
  await expect(review.getByLabel("Comment")).toHaveValue("");
  await review.getByLabel("Comment").fill("Check this specific behavior.");
  await review.getByRole("button", { name: "Suggest replacement" }).click();
  await review.getByLabel("Replacement code").fill("    // proposed replacement");
  await review.getByRole("button", { name: "Preview GitHub suggestion" }).click();
  await expect(review.getByText("Check this specific behavior.", { exact: false }).last()).toBeVisible();
  await expect(review.getByText("    // proposed replacement", { exact: false })).toBeVisible();
  await expect(review.getByText("Publishing requires write access to pull requests.")).toBeVisible();
  await expect(review.getByRole("link", { name: "Manage repository access" })).toHaveAttribute("href", installUrl);
  expect(posts).toHaveLength(2);
  await review.getByRole("button", { name: "Publish to GitHub" }).click();
  await expect(review.getByRole("link", { name: "Published on GitHub" })).toBeVisible();
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
  const thread = review.getByRole("button", { name: /Comment.*demo\.rs:25.*For you/ });
  await expect(thread).toBeVisible();
  const resolved = review.getByRole("button", { name: /demo\.rs:26.*Resolved/ });
  await expect(resolved).toHaveCount(0);
  await review.getByRole("checkbox", { name: "Show resolved threads" }).check();
  await expect(resolved).toBeVisible();
  await review.getByRole("button", { name: "Fly to next mention" }).click();
  await expect(review).toBeHidden();
  await lock();
  await page.keyboard.press("KeyJ", { delay: 120 });
  await expect(review.getByRole("heading", { name: "Review · PR #8" })).toBeHidden();
  await expect.poll(async () => {
    const title = await review.evaluate(dialog => dialog.open ? dialog.querySelector("h2").textContent : null);
    if (title === "src/demo.rs:25") return true;
    if (title !== null) {
      await review.getByRole("button", { name: "Close", exact: true }).click();
      return false;
    }
    if (!await page.evaluate(() => Boolean(document.pointerLockElement))) await lock();
    await page.keyboard.press("KeyJ", { delay: 120 });
    return false;
  }, { timeout: 20_000 }).toBe(true);
  await expect(review.getByText("Separate note")).toBeVisible();
  await expect(review.getByText("Got it, @Reviewer. Check the line above.")).toBeVisible();
  await review.getByRole("button", { name: "Mark viewed on GitHub 👀" }).click();
  await expect(review.locator("article").getByRole("status")).toContainText("Pull requests: write");
  await review.getByRole("button", { name: "Mark viewed on GitHub 👀" }).click();
  await expect(review.getByText("Viewed on GitHub 👀")).toBeVisible();
  await review.getByLabel("Local paint").selectOption({ label: "Investigate" });
  await review.getByRole("button", { name: "Save local paint" }).click();
  await expect(review.getByRole("button", { name: /Investigate.*demo\.rs:25.*GitHub thread/ })).toBeVisible();
  expect(posts).toHaveLength(3);
  await page.reload();
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect.poll(() => page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes
    .filter(note => note.remote).map(note => [note.id, note.preset.label, note.resolved]))).toEqual([
    ["github-13", "Investigate", false], ["github-15", "Comment", true], ["github-42", "Investigate", null],
  ]);
  expect(await page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes
    .find(note => note.id === "github-13").attention)).toBe(false);
  expect(reactionAttempts).toBe(2);
});
