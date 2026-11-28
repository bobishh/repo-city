import { test, expect } from "@playwright/test";
import { expectClosed, expectOpen, gpuClick, servicePanel } from "./gpu-tablet.mjs";

test("keeps the world selector behind the preview flag", async ({ page }) => {
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  const selector = page.locator(".world-picker");
  await page.goto("/");
  await expect(selector).toHaveJSProperty("hidden", true);
  await page.goto("/?world-picker=1");
  await expect(selector).toHaveJSProperty("hidden", false);
  await expect(selector.locator("option:checked")).toHaveText("Code City");
});

test("styles the shared repository form for the selected world", async ({ page }) => {
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  const form = page.getByRole("form", { name: "Open repository" });
  await page.goto("/?world=repo-polis");
  await expect(form).toHaveCSS("background-color", "rgb(37, 29, 23)");
  const search = form.locator("#repository-query");
  await search.focus({ timeout: 5000 });
  await expect(form.locator("#repository-search .picker-search")).toHaveCSS("border-bottom-color", "rgb(211, 165, 104)");
  await expect(page.locator(".library-title")).toBeVisible();
  await expect(page.locator(".library-title")).toHaveText("Repo Polis");
  await expect(page).toHaveTitle("Repo Polis · Repo City Web");
  await expect(page.locator("#start-screen img")).toHaveAttribute("src", "./assets/repo-polis-start.webp");
  await expect.poll(() => page.locator("#start-screen img").evaluate(image => image.naturalWidth)).toBeGreaterThan(1000);
  const colors = await form.locator("#open-city").evaluate(button => {
    const style = getComputedStyle(button);
    return [style.color, style.backgroundColor];
  });
  expect(colors).toEqual(["rgb(33, 21, 10)", "rgb(211, 165, 104)"]);
  await page.goto("/?world=greek-library");
  await expect(page.locator("html")).toHaveAttribute("data-world", "code-city");
  await expect(form).toHaveCSS("background-color", "rgb(10, 19, 26)");
});

test("fits every file and travels between folders before opening a distant scroll", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1900, height: 732 });
  const revision = "c".repeat(40);
  const tree = Array.from({ length: 6000 }, (_, index) => ({
    path: `gallery-${String(index % 100).padStart(2, "0")}/file-${String(index).padStart(4, "0")}.rs`,
    type: "blob",
    mode: "100644",
    sha: (index + 10).toString(16).padStart(40, "0"),
    size: 15,
  }));
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/library") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: revision, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree } });
    return route.fulfill({ json: { encoding: "base64", content: Buffer.from("pub struct Roll;\n").toString("base64") } });
  });
  await page.goto("/?repo=owner%2Flibrary&world=repo-polis");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("#repo-city");
  await canvas.focus();
  const press = key => page.keyboard.press(key, { delay: 135 });
  await press("KeyM");
  await expect(canvas).toHaveAttribute("data-library-map", "Folder · gallery-00");
  await expect(canvas).toHaveAttribute("data-library-map-files", "60");
  await page.mouse.move(950, 366);
  await expect(canvas).toHaveAttribute("data-library-map-hover", "gallery-00");
  await page.screenshot({ path: testInfo.outputPath("folder-hover.png") });
  await page.mouse.move(950, 722);
  await expect(canvas).not.toHaveAttribute("data-library-map-hover");
  await page.screenshot({ path: testInfo.outputPath("folder.png") });
  await press("KeyM");
  await expect(canvas).toHaveAttribute("data-library-map", "Archive courtyards");
  await expect(canvas).toHaveAttribute("data-library-map-files", "6000");
  const view = { x: 380, y: 110, width: 1140, height: 480 };
  const firstProjectFrame = await page.screenshot({ clip: view });
  await page.waitForTimeout(220);
  const laterProjectFrame = await page.screenshot({ clip: view });
  expect(firstProjectFrame.equals(laterProjectFrame)).toBe(false);
  await page.screenshot({ path: testInfo.outputPath("archive.png") });
  await press("End");
  await press("Enter");
  await expect(canvas).toHaveAttribute("data-library-travel", "gallery-99");
  await expect(canvas).not.toHaveAttribute("data-library-reading");
  await press("Escape");
  await expect(canvas).not.toHaveAttribute("data-library-travel");
  await expect(canvas).toHaveAccessibleName("Repo Polis");
  await press("KeyM");
  await expect(canvas).toHaveAttribute("data-library-map", "Folder · gallery-00");
  await press("KeyM");
  await press("End");
  await press("Enter");
  await expect(canvas).toHaveAttribute("data-library-travel", "gallery-99");
  await expect(canvas).toHaveAttribute("data-library-map", "Archive courtyards");
  await page.waitForTimeout(900);
  await expect(canvas).toHaveAttribute("data-library-travel", "gallery-99");
  await expect(canvas).not.toHaveAttribute("data-library-reading");
  await page.screenshot({ path: testInfo.outputPath("crossing.png") });
  await expect(canvas).not.toHaveAttribute("data-library-map");
  await expect(canvas).not.toHaveAttribute("data-library-travel");
  await expect(canvas).toHaveAttribute("data-library-flight", "landing");
  await expect(canvas).not.toHaveAttribute("data-library-reading");
  await expect(canvas).not.toHaveAttribute("data-library-flight");
  await press("Enter");
  await expect(canvas).toHaveAccessibleName("Repo Polis, gallery-99/file-5999.rs, lines 1–1");
  await press("Escape");
  await expect(canvas).toHaveAccessibleName("Repo Polis");
  await press("KeyM");
  await expect(canvas).toHaveAttribute("data-library-map", "Folder · gallery-99");
  await expect(canvas).toHaveAttribute("data-library-map-files", "60");
  await press("Home");
  await press("Enter");
  await expect(canvas).toHaveAttribute("data-library-travel", "gallery-99");
  await expect(canvas).toHaveAttribute("data-library-map", "Folder · gallery-99");
  await expect(canvas).not.toHaveAttribute("data-library-reading");
  await press("Escape");
  await expect(canvas).not.toHaveAttribute("data-library-map-files");
  await expect(canvas).not.toHaveAttribute("data-library-travel");
});

test("opens a repository in Repo Polis and unrolls a file at the requested line", async ({ page }, testInfo) => {
  page.on("pageerror", error => console.error(error));
  page.on("console", message => { if (message.type() === "error") console.error(message.text()); });
  const revision = "a".repeat(40);
  const document = "b".repeat(40);
  const source = Array.from({ length: 2400 }, (_, row) => `pub const ROW_${row + 1}: usize = ${row + 1};\n`).join("");
  let slowSource = false;
  await page.addInitScript(() => {
    if (!localStorage.getItem("repo-city-reading-v1")) {
      localStorage.setItem("repo-city-reading-v1", JSON.stringify({ enabled: true, block_kind: "lines", min_seconds: 0.1, max_seconds: 0.2 }));
    }
  });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/library") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: revision, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: [
      { path: "README.md", type: "blob", mode: "100644", sha: revision, size: 9 },
      { path: "src/scroll.rs", type: "blob", mode: "100644", sha: document, size: source.length },
    ] } });
    if (slowSource && path.endsWith(document)) await new Promise(resolve => setTimeout(resolve, 750));
    const body = Buffer.from(path.endsWith(document) ? source : "# Library");
    return route.fulfill({ json: { size: body.length, encoding: "base64", content: body.toString("base64") } });
  });
  await page.goto("/?repo=owner%2Flibrary&world=repo-polis");
  const canvas = page.locator("#repo-city");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect(canvas).toHaveAccessibleName("Repo Polis");
  await expect(page).toHaveURL(/world=repo-polis/);
  await canvas.focus();
  await canvas.click({ button: "right", position: { x: 480, y: 270 } });
  await expect(canvas).toHaveAttribute("data-action-menu", /Find file/);
  await expect(canvas).not.toHaveAttribute("data-pointer-locked");
  await page.keyboard.press("Escape");
  await expect(canvas).not.toHaveAttribute("data-action-menu");
  await page.keyboard.press("Space");
  await expect(canvas).toHaveAttribute("data-library-flight", "flying");
  await page.keyboard.press("KeyO");
  await expect(canvas).toHaveAttribute("data-library-flight", "landing");
  await expect(canvas).not.toHaveAttribute("data-library-flight");
  await expect(page.locator("#city-progress")).toBeHidden();
  await expect(page.locator(".wordmark")).toHaveCSS("font-family", /GFS Didot/);
  await canvas.focus();
  // The reading room stays navigable and legible on its shaded side.
  await page.screenshot({ path: testInfo.outputPath("library.png"), style: "#city-header { visibility: hidden }" });
  await page.keyboard.press("ArrowLeft", { delay: 450 });
  await page.screenshot({ path: testInfo.outputPath("shelf-labels.png"), style: "#city-header { visibility: hidden }" });

  const press = code => page.keyboard.press(code, { delay: 135 });
  await press("Slash");
  await page.keyboard.type("no_such_file", { delay: 65 });
  await press("Enter");
  await expect(canvas).toHaveAccessibleName("Repo Polis");
  await press("Slash");
  await page.keyboard.type("scroll.rs", { delay: 65 });
  await press("Enter");
  await expect(canvas).toHaveAccessibleName("Repo Polis, src/scroll.rs, lines 1–22", { timeout: 30_000 });
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
  await page.mouse.move(490, 350);
  await expect(canvas).toHaveAttribute("data-library-reticle", "");
  await canvas.click({ button: "right", position: { x: 490, y: 350 } });
  await expect(canvas).toHaveAttribute("data-action-menu", /Open scroll/);
  await page.keyboard.press("b");
  await expect(canvas).not.toHaveAttribute("data-action-menu");
  await expect(canvas).toHaveAttribute("data-library-face", "blame");
  await page.keyboard.press("b");
  await expect(canvas).toHaveAttribute("data-library-face", "source");
  await page.screenshot({ path: testInfo.outputPath("printed-scroll.png") });
  const reveals = () => page.evaluate(async () => {
    const db = await new Promise((resolve, reject) => {
      const request = indexedDB.open("repo-city-progress", 2);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    const result = await new Promise((resolve, reject) => {
      const request = db.transaction("marks").objectStore("marks").getAll();
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    db.close();
    return result.map(value => JSON.parse(value)).filter(mark => mark.reveal?.path === "src/scroll.rs").length;
  });
  expect(await reveals()).toBe(0);
  await press("KeyK");
  await expect.poll(reveals).toBeGreaterThan(0);
  await press("KeyK");
  await canvas.focus();
  await press("KeyJ");
  await expect(canvas).toHaveAccessibleDescription("Enter a line number. Press Enter to go.");
  await page.keyboard.type("1750", { delay: 100 });
  await press("Enter");
  await expect(canvas).toHaveAccessibleName("Repo Polis, src/scroll.rs, lines 1750–1771");
  await expect.poll(visibleInk, { timeout: 7500 }).toBeGreaterThan(150);
  await page.screenshot({ path: testInfo.outputPath("scroll.png") });
  await press("End");
  await expect(canvas).toHaveAccessibleName("Repo Polis, src/scroll.rs, lines 2379–2400");
  await press("Backslash");
  await expect(canvas).toHaveAccessibleDescription("Find text in this scroll. Press Enter to search.");
  await expect(canvas).toHaveAccessibleName("Repo Polis, src/scroll.rs, lines 2379–2400");
  await expect.poll(visibleInk, { timeout: 7500 }).toBeGreaterThan(150);
  await page.keyboard.type("ROW_1800", { delay: 55 });
  await press("Enter");
  await expect(canvas).toHaveAccessibleName("Repo Polis, src/scroll.rs, lines 1800–1821");
  await expect.poll(() => page.evaluate(() => {
    const key = Object.keys(localStorage).find(key => key.includes("|world=repo-polis"));
    return key ? JSON.parse(localStorage.getItem(key)).line : 0;
  })).toBe(1800);
  slowSource = true;
  await page.reload();
  await expect(canvas).toHaveAccessibleName("Repo Polis, src/scroll.rs, lines 1800–1821", { timeout: 30_000 });
  await canvas.focus();
  await press("KeyM");
  await expect(canvas).toHaveAttribute("data-library-map", "Folder · src");
  await press("Escape");
  await expect(canvas).toHaveAccessibleName("Repo Polis, src/scroll.rs, lines 1800–1821");
  await press("KeyS");
  await expect(canvas).toHaveAccessibleName("Repo Polis");
  await press("KeyM");
  await expect(canvas).toHaveAttribute("data-library-map", "Folder · src");
  await press("KeyM");
  await expect(canvas).toHaveAttribute("data-library-map", "Archive courtyards");
  await page.screenshot({ path: testInfo.outputPath("corridors.png"), style: "#city-header { visibility: hidden }" });
  await press("End");
  await press("Enter");
  await expect(canvas).toHaveAttribute("data-library-travel", "src");
  await expect(canvas).not.toHaveAttribute("data-library-reading");
  await expect(canvas).not.toHaveAttribute("data-library-travel");
  await expect(canvas).not.toHaveAttribute("data-library-map");
  await expect(canvas).toHaveAttribute("data-library-flight", "landing");
  await expect(canvas).not.toHaveAttribute("data-library-reading");
  await expect(canvas).not.toHaveAttribute("data-library-flight");
  await press("Enter");
  await expect(canvas).toHaveAccessibleName("Repo Polis, src/scroll.rs, lines 1–22");
  await press("Escape");
  await press("Tab");
  await expectOpen(page, "Changed files");
  const file = servicePanel(page).getByRole("button", { name: /src\/scroll\.rs/, includeHidden: true });
  await gpuClick(page, file);
  await expectClosed(page);
  await expect(canvas).toHaveAccessibleName("Repo Polis, src/scroll.rs, lines 1–22");
});

test("paints a private scroll range by dragging and marks its file on the map", async ({ page }, testInfo) => {
  const base = "a".repeat(40);
  const head = "b".repeat(40);
  const path = "src/paint.rs";
  const source = Array.from({ length: 36 }, (_, index) => `pub const ROW_${index + 1}: u32 = ${index + 1};\n`).join("");
  await page.addInitScript(() => localStorage.setItem("repo-city-reading-v1", JSON.stringify({
    enabled: false,
    show_all_source: true,
  })));
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: true, login: "reviewer" } }));
  await page.route(/\/github\/api\/|https:\/\/api.github.com\//, route => {
    const request = new URL(route.request().url()).pathname.replace("/github/api", "");
    if (request.endsWith("/pulls/8")) return route.fulfill({ json: { state: "open", base: { sha: base }, head: { sha: head } } });
    if (request.includes("/compare/")) return route.fulfill({ json: { merge_base_commit: { sha: base } } });
    if (request.includes("/git/trees/")) return route.fulfill({ json: { tree: [{
      path, type: "blob", mode: "100644", sha: request.endsWith(head) ? head : base, size: source.length,
    }] } });
    if (request.includes("/git/blobs/")) {
      const text = request.endsWith(head) ? source : source.replace("= 4;", "= 40;");
      return route.fulfill({ json: { size: text.length, encoding: "base64", content: Buffer.from(text).toString("base64") } });
    }
    return route.fulfill({ json: [] });
  });
  let publications = 0;
  await page.route("**/github/review/owner/library/8", route => {
    if (route.request().method() === "POST") publications++;
    return route.fulfill({ json: { headRefOid: head, reviewThreads: { nodes: [], pageInfo: { hasNextPage: false } } } });
  });
  await page.goto("/?world=repo-polis&pr=https%3A%2F%2Fgithub.com%2Fowner%2Flibrary%2Fpull%2F8");
  const canvas = page.locator("#repo-city");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect.poll(() => page.evaluate(() => globalThis.repoCity.reviewCanPaint())).toBe(true);
  await canvas.focus();
  await page.keyboard.press("Enter", { delay: 135 });
  await expect(canvas).toHaveAccessibleName(/src\/paint\.rs, lines 1–22/);
  await page.waitForTimeout(3500);
  const frame = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const aim = async y => {
    await page.mouse.move(540, y);
    await frame();
    await expect(canvas).toHaveAttribute("data-library-target", /^Line \d+$/);
    return Number((await canvas.getAttribute("data-library-target")).slice(5));
  };
  const first = await aim(350);
  await page.keyboard.press("Digit2", { delay: 135 });
  await page.mouse.down();
  await frame();
  await page.mouse.move(540, 490, { steps: 12 });
  await expect.poll(() => canvas.getAttribute("data-library-target")).not.toBe(`Line ${first}`);
  const last = await aim(490);
  await page.mouse.up();
  await expect(canvas).not.toHaveAttribute("data-pointer-locked");
  const notes = () => page.evaluate(() => JSON.parse(globalThis.repoCity.reviewRead()).notes
    .filter(note => !note.remote).map(note => note.anchor));
  await expect.poll(notes).toEqual([{ path, before: false, start: Math.min(first, last), end: Math.max(first, last) }]);
  expect(publications).toBe(0);
  await page.screenshot({ path: testInfo.outputPath("painted-scroll.png") });
  await page.keyboard.press("KeyM", { delay: 135 });
  await expect(canvas).toHaveAttribute("data-library-map", "Folder · src");
  await expect(canvas).toHaveAttribute("data-library-note-pins", "1");
  await page.screenshot({ path: testInfo.outputPath("painted-map.png") });
});
