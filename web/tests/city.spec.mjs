import { test, expect } from "@playwright/test";
import { writeFile } from "node:fs/promises";

const after = "b".repeat(40);
const before = "a".repeat(40);
const next = "e".repeat(40);

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
  await expect(page.getByRole("button", { name: "Repo City" })).toBeVisible();
  await expect(page.getByRole("textbox", { name: "GitHub repository" })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("landing.png") });
  const syntax = page.waitForResponse(response => new URL(response.url()).pathname === "/syntax-worker.mjs");
  await expect.poll(() => cover.locator("img").evaluate(image => image.naturalWidth)).toBeGreaterThan(0);
  await page.getByRole("textbox", { name: "GitHub repository" }).fill("owner/demo");
  const blob = page.waitForResponse(response => response.url().includes("/git/blobs/"), { timeout: 75_000 }).catch(() => null);
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.locator("#status")).toBeEmpty({ timeout: 70_000 });
  expect(await blob).not.toBeNull();
  await expect(page.locator("form")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas");
  await expect(canvas).toBeVisible();
  await expect(cover).toBeHidden();
  await expect.poll(() => canvas.evaluate(element => element.getBoundingClientRect().top)).toBe(0);
  expect(await syntax).not.toBeNull();
  await expect.poll(() => page.evaluate(() => globalThis.repoCity.syntaxVersion())).toBeGreaterThan(0);
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
  await page.getByRole("button", { name: "Repo City" }).click();
  const dialog = page.getByRole("dialog", { name: "Open repository" });
  await expect(dialog).toBeVisible();
  await expect(canvas).toBeVisible();
  await page.route("https://api.github.com/repos/owner/missing", route => route.fulfill({ status: 404 }));
  await dialog.getByRole("textbox", { name: "GitHub repository" }).fill("owner/missing");
  await dialog.getByRole("button", { name: "Open city" }).click();
  await expect(dialog.getByRole("status")).toHaveText("Repository, revision, or file not found. Private repositories need GitHub access.");
  await expect(canvas).toBeVisible();
  await dialog.getByRole("textbox", { name: "GitHub repository" }).fill("owner/demo");
  await dialog.getByLabel("Branch, tag, or commit", { exact: true }).fill(next);
  const committed = page.waitForRequest(request => request.url().endsWith("/commits/" + next));
  const replacement = page.waitForResponse(response => response.url().endsWith("/git/blobs/" + next));
  await dialog.getByRole("button", { name: "Open city" }).click();
  await committed;
  expect((await replacement).ok()).toBe(true);
  await expect(dialog).toBeHidden();
  await expect(page).toHaveURL(new RegExp("ref=" + next));
  await page.getByRole("button", { name: "Repo City" }).click();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
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
  await canvas.dispatchEvent("pointerdown", { button: 0 });
  expect(await page.evaluate(() => document.pointerLockElement?.id)).toBe("repo-city");

});

test("reports missing repositories and keeps the form usable", async ({ page }) => {
  await page.route("https://api.github.com/**", route => route.fulfill({ status: 404 }));
  await page.goto("/");
  await page.getByRole("textbox", { name: "GitHub repository" }).fill("owner/missing");
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.getByRole("status")).toHaveText("Repository, revision, or file not found. Private repositories need GitHub access.");
  await expect(page.getByRole("button", { name: "Open city" })).toBeEnabled();
  await expect(page.getByRole("button", { name: "Repo City" })).toBeVisible();
});

test("opens a pull request from the URL using its fork head and merge base", async ({ page }, testInfo) => {
  const checked = [];
  await page.route("https://api.github.com/**", async route => {
    const path = new URL(route.request().url()).pathname;
    checked.push(path);
    if (path.endsWith("/pulls/8")) return route.fulfill({ json: {
      state: "open",
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
});


test("offers GitHub libraries and applies a commit suggestion to the comparison base", async ({ page }) => {
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: true, login: "owner" } }));
  await page.route("**/github/api/**", route => {
    const path = new URL(route.request().url()).pathname;
    const values = {
      "/github/api/users/owner/repos": [{ full_name: "owner/fork", fork: true }],
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
  const group = page.getByLabel("GitHub list");
  const repos = page.getByLabel("Repository from GitHub");
  await expect(repos.locator("option", { hasText: "owner/secret · Private" })).toHaveCount(1);
  await group.selectOption("stars");
  await expect(repos.locator("option", { hasText: "other/star" })).toHaveCount(1);
  await group.selectOption("forks");
  await expect(repos.locator("option", { hasText: "owner/fork" })).toHaveCount(1);
  await group.selectOption("repositories");
  await repos.selectOption("owner/secret");
  await expect(page.getByRole("textbox", { name: "GitHub repository" })).toHaveValue("owner/secret");
  const revisions = page.getByLabel("Revision suggestions");
  await expect(revisions.locator("option", { hasText: "Paint the robot" })).toHaveCount(1);
  await revisions.selectOption("refs/heads/feature/robot");
  await expect(page.getByLabel("Branch, tag, or commit", { exact: true })).toHaveValue("refs/heads/feature/robot");
  await page.getByLabel("Use as").selectOption("base");
  await revisions.selectOption(before);
  await expect(page.getByLabel("Compare with", { exact: true })).toHaveValue(before);
  const blob = page.waitForResponse(response => response.url().endsWith("/github/api/repos/owner/secret/git/blobs/" + after));
  await page.getByRole("button", { name: "Open city" }).click();
  expect((await blob).ok()).toBe(true);
  await expect(page.locator("form")).toBeHidden({ timeout: 90_000 });
});

test("restores review progress after reload and resets only the open comparison", async ({ page }) => {
  await page.route("https://api.github.com/**", async route => {
    const path = new URL(route.request().url()).pathname;
    const source = Array.from({ length: 30 }, (_, i) => `// row ${i}\n`).join("");
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [{ sha: before }] } });
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
  await expect.poll(async () => {
    await page.keyboard.down("KeyF");
    await page.waitForTimeout(350);
    await page.keyboard.up("KeyF");
    return (await read(scope)).some(mark => JSON.parse(mark).reveal);
  }, polling).toBe(true);
  const marks = await read(scope);
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

  await page.getByRole("button", { name: "Repo City" }).click();
  await expect(summary).toHaveText(/1 file reviewed · [1-9][0-9]* lines revealed/);
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
});
