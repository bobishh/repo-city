import { expect, test } from "@playwright/test";
import { expectClosed, expectOpen, gpuClick, servicePanel } from "./gpu-tablet.mjs";

async function canvasClick(page, label) {
  const scene = page.locator("#repo-city");
  const point = async () => {
    const controls = JSON.parse(await scene.getAttribute("data-diff-canvas-controls") ?? "[]");
    return controls.find(control => typeof label === "string" ? control.label === label : label.test(control.label));
  };
  await expect.poll(async () => Boolean(await point()), { message: `Find canvas control ${label}` }).toBe(true);
  const { x, y } = await point();
  const bounds = await scene.boundingBox();
  await page.mouse.click(bounds.x + x * bounds.width, bounds.y + y * bounds.height, { delay: 100 });
}

test("groups selected repository files in a named canvas and adds the aimed scroll", async ({ page }) => {
  const revision = "a".repeat(40);
  const base = "b".repeat(40);
  const unchanged = ["src/first.rs", "src/second.rs", "src/scroll.rs"];
  const source = Array.from({ length: 120 }, (_, row) => `pub const ROW_${row + 1}: usize = ${row + 1};\n`).join("");
  const entry = (path, sha) => ({ path, sha, mode: "100644", type: "blob", size: source.length });
  await page.addInitScript(() => {
    document.addEventListener("DOMContentLoaded", () => document.documentElement.setAttribute("data-canvas-inspection", ""), { once: true });
    localStorage.setItem("repo-city-guide-v1", "seen");
  });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/collection") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: revision, parents: [{ sha: base }] } });
    if (path.includes("/git/trees/")) {
      return route.fulfill({ json: { tree: [
        entry("src/changed.rs", path.endsWith(revision) ? revision : base),
        ...unchanged.map((name, index) => entry(name, String(index + 1).repeat(40))),
      ] } });
    }
    return route.fulfill({ json: { encoding: "base64", size: source.length, content: Buffer.from(source).toString("base64") } });
  });

  await page.goto("/?repo=owner%2Fcollection&world=repo-polis");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const scene = page.locator("#repo-city");
  const press = key => page.keyboard.press(key, { delay: 135 });
  await scene.focus();
  await press("Tab");
  await expectOpen(page, "Changed files");
  const choose = servicePanel(page).getByRole("button", { name: "Choose files for canvas", exact: true, includeHidden: true });
  await gpuClick(page, choose);

  // Select two unchanged paths and create a separate canvas for them.
  await canvasClick(page, /^□\s+src\/first\.rs$/);
  await canvasClick(page, /^□\s+src\/second\.rs$/);
  await canvasClick(page, "Add 2 files");
  await canvasClick(page, "New canvas…");
  await canvasClick(page, "Create");
  await expect(scene).toHaveAttribute("data-diff-canvas-status", "Enter a canvas name.");
  await page.keyboard.type("Inspection notes", { delay: 55 });
  await canvasClick(page, "Create");
  await expect(scene).toHaveAttribute("data-diff-canvas", /src\/first\.rs/);
  await expect(scene).toHaveAttribute("data-diff-canvas", /src\/second\.rs/);
  await expect(scene).not.toHaveAttribute("data-diff-canvas", /src\/changed\.rs/);
  await press("Escape");
  await expect(scene).not.toHaveAttribute("data-diff-canvas");
  await expectOpen(page, "Changed files");
  await press("Tab");
  await expectClosed(page);

  // Add exactly the scroll under the cursor using the existing right-click menu.
  await scene.focus();
  await press("Slash");
  await page.keyboard.type("scroll.rs", { delay: 65 });
  await press("Enter");
  await expect(scene).toHaveAccessibleName(/Repo Polis, src\/scroll\.rs, lines/);
  await page.mouse.move(490, 350);
  await expect(scene).toHaveAttribute("data-library-reticle");
  await expect(scene).toHaveAttribute("data-library-target", /^Line /);
  await scene.click({ button: "right", position: { x: 490, y: 350 } });
  await expect(scene).toHaveAttribute("data-action-menu", /Open scroll/);
  for (let row = 0; row < 10; row++) {
    const current = await scene.getAttribute("data-action-menu");
    if (current === "Add to canvas…") break;
    await press("ArrowDown");
    await expect(scene).not.toHaveAttribute("data-action-menu", current);
  }
  await expect(scene).toHaveAttribute("data-action-menu", "Add to canvas…");
  await press("Enter");
  await canvasClick(page, "Inspection notes (2)");
  await expect(scene).not.toHaveAttribute("data-diff-canvas");
  await press("Tab");
  await expectOpen(page, "Changed files");
  const open = servicePanel(page).getByRole("button", { name: "Open canvas", exact: true, includeHidden: true });
  await gpuClick(page, open);
  await canvasClick(page, "Canvases");
  await canvasClick(page, "Inspection notes (3)");
  await expect(scene).toHaveAttribute("data-diff-canvas", /src\/scroll\.rs/);
  await expect(scene).toHaveAttribute("data-diff-canvas", /src\/first\.rs/);
  await expect(scene).toHaveAttribute("data-diff-canvas", /src\/second\.rs/);
  await expect(scene).not.toHaveAttribute("data-diff-canvas", /src\/changed\.rs/);
});

test("loads every visible diff and arranges the overview for the window", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 2130, height: 900 });
  const revision = "a".repeat(40);
  const base = "b".repeat(40);
  const body = before => Array.from({ length: 48 }, (_, row) =>
    `const ROW_${row}: usize = ${before && [4, 40].includes(row) ? row + 200 : row};`
  ).join("\n") + "\n";
  const contents = new Map();
  const tree = before => Array.from({ length: 14 }, (_, index) => {
    const sha = (index + (before ? 100 : 200)).toString(16).padStart(40, "0");
    const source = body(before);
    contents.set(sha, source);
    return { path: `file_${String(index).padStart(2, "0")}.rs`, sha, mode: "100644", type: "blob", size: source.length };
  });
  const headTree = tree(false), baseTree = tree(true);
  const requested = new Set();
  await page.addInitScript(() => {
    document.addEventListener("DOMContentLoaded", () => document.documentElement.setAttribute("data-canvas-inspection", ""), { once: true });
    localStorage.setItem("repo-city-guide-v1", "seen");
  });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/diff-workload") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: revision, parents: [{ sha: base }] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: path.endsWith(revision) ? headTree : baseTree } });
    const sha = path.split("/").at(-1);
    const source = contents.get(sha);
    if (!source) return route.fulfill({ json: [] });
    requested.add(sha);
    return route.fulfill({ json: { encoding: "base64", size: source.length, content: Buffer.from(source).toString("base64") } });
  });

  await page.goto("/?repo=owner%2Fdiff-workload");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const scene = page.locator("#repo-city");
  await scene.focus();
  await page.keyboard.press("Tab", { delay: 135 });
  await expectOpen(page, "Changed files");
  const open = servicePanel(page).getByRole("button", { name: "Open canvas", exact: true, includeHidden: true });
  await gpuClick(page, open);

  // Each visible tile renders a hunk navigation button only after parsing both blobs.
  await expect.poll(async () => {
    const buttons = JSON.parse(await scene.getAttribute("data-diff-canvas-controls") ?? "[]");
    return buttons.filter(button => button.label === "hunk ›").length;
  }, { timeout: 30_000 }).toBeGreaterThanOrEqual(6);
  expect(requested.has(headTree.at(-1).sha)).toBe(false);
  expect(requested.has(baseTree.at(-1).sha)).toBe(false);

  const contextButtons = () => scene.evaluate(element =>
    JSON.parse(element.dataset.diffCanvasControls ?? "[]").filter(button => button.label === "Show 31 unchanged lines")
  );
  await expect.poll(async () => (await contextButtons()).length).toBeGreaterThanOrEqual(6);
  const beforeExpansion = await contextButtons();
  await canvasClick(page, "Show 31 unchanged lines");
  await expect.poll(async () => (await contextButtons()).length).toBeLessThan(beforeExpansion.length);

  await page.setViewportSize({ width: 2130, height: 620 });
  await canvasClick(page, "Show all");
  const geometry = async () => JSON.parse(await scene.getAttribute("data-diff-canvas-layout"));
  await expect.poll(async () => (await geometry()).overview).toBe(true);
  const { tiles } = await geometry();
  expect(tiles).toHaveLength(14);
  expect(tiles.every(([x, y, w, h]) => x >= 0 && y >= 0 && x + w <= 2130.01 && y + h <= 620.01)).toBe(true);
  const area = tiles.reduce((total, [, , width, height]) => total + width * height, 0);
  expect(area / (2130 * (620 - 44))).toBeGreaterThan(0.88);
  const lastRow = tiles.filter(tile => tile[1] === tiles.at(-1)[1]);
  expect(lastRow.at(-1)[0] + lastRow.at(-1)[2]).toBeGreaterThan(2100);
  await expect.poll(() => requested.has(headTree.at(-1).sha) && requested.has(baseTree.at(-1).sha)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("overview.png") });
});
