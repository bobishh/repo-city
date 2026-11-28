import { test, expect } from "@playwright/test";
import { expectOpen, gpuClick, servicePanel } from "./gpu-tablet.mjs";

for (const world of ["code-city", "repo-polis"]) {
  test(`opens the same comparison on the fullscreen canvas in ${world} and returns to Files`, async ({ page }, testInfo) => {
    const revision = "b".repeat(40);
    const base = "a".repeat(40);
    const treeEntry = (path, sha) => ({ path, sha, mode: "100644", type: "blob", size: 20 });
    await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
    const pull = world === "repo-polis";
    await page.route("**/github/session", route => route.fulfill({ json: pull ? { enabled: true, login: "reviewer" } : { enabled: false } }));
    await page.route(/\/github\/api\/|https:\/\/api.github.com\//, route => {
      const path = new URL(route.request().url()).pathname.replace("/github/api", "");
      if (path.endsWith("/pulls/8")) return route.fulfill({ json: { state: "open", base: { sha: base }, head: { sha: revision } } });
      if (path.includes("/compare/")) return route.fulfill({ json: { merge_base_commit: { sha: base } } });
      if (path.endsWith("/comments")) return route.fulfill({ json: [] });
      if (path === "/repos/owner/canvas") return route.fulfill({ json: { default_branch: "main" } });
      if (path.includes("/commits/")) return route.fulfill({ json: { sha: revision, parents: [{ sha: base }] } });
      if (path.includes("/git/trees/")) {
        const head = path.endsWith(revision);
        return route.fulfill({ json: { tree: [
          treeEntry("src/light.rs", head ? revision : base),
          treeEntry("src/stable.rs", "d".repeat(40)),
          ...head ? [] : [treeEntry("old.rs", "c".repeat(40))],
        ] } });
      }
      const source = path.endsWith(revision) ? "const SHADOW: bool = false;\n" : "const SHADOW: bool = true;\n";
      return route.fulfill({ json: { encoding: "base64", size: source.length, content: Buffer.from(source).toString("base64") } });
    });
    const comparison = pull ? "pr=https%3A%2F%2Fgithub.com%2Fowner%2Fcanvas%2Fpull%2F8" : "repo=owner%2Fcanvas";
    await page.goto(`/?${comparison}&world=${world}`);
    await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
    const scene = page.locator("#repo-city");
    await scene.focus();
    await page.keyboard.press("Tab", { delay: 120 });
    await expectOpen(page, "Changed files");
    const open = servicePanel(page).getByRole("button", { name: "Open canvas", exact: true, includeHidden: true });
    await gpuClick(page, open);
    await expect(scene).toHaveAttribute("data-diff-canvas", /src\/light\.rs/);
    await expect(scene).toHaveAttribute("data-diff-canvas", /old\.rs/);
    await expect(scene).not.toHaveAttribute("data-diff-canvas", /stable\.rs/);
    await expect(page.locator("#city-header")).toBeHidden();
    await expect(scene).toHaveAttribute("data-diff-canvas-controls", /Show all/);
    const layout = async () => JSON.parse(await scene.getAttribute("data-diff-canvas-layout"));
    const drag = async (x, y, dx, dy) => {
      await page.mouse.move(x, y);
      const frame = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      await page.mouse.down();
      await frame();
      await page.mouse.move(x + dx, y + dy, { steps: 8 });
      await frame();
      await page.mouse.up();
    };
    const clickControl = async label => {
      let control;
      await expect.poll(async () => {
        const controls = JSON.parse(await scene.getAttribute("data-diff-canvas-controls") ?? "[]");
        control = controls.find(item => item.label === label);
        return Boolean(control);
      }).toBe(true);
      await page.mouse.click(control.x * 1280, control.y * 800, { delay: 100 });
    };
    if (!pull) {
      const original = await layout();
      const [x, y, width, height] = original.tiles[0];
      await drag(x + 70, y + 12, 75, 43);
      await expect.poll(async () => (await layout()).pan[0]).toBeLessThan(original.pan[0] - 20);
      await page.keyboard.press("ArrowRight", { delay: 180 });
      await expect.poll(async () => (await layout()).pan[0]).toBeGreaterThan(original.pan[0] - 65);
      await clickControl("Show all");
      await expect.poll(async () => (await layout()).overview).toBe(true);
      await clickControl("Reading view");
      await expect.poll(async () => (await layout()).overview).toBe(false);
      const [left, top] = (await layout()).tiles[0];
      await drag(left + width - 6, top + height - 6, 160, 75);
      await expect.poll(async () => (await layout()).tiles[0][2]).toBeGreaterThan(width + 75);
      await expect.poll(async () => (await layout()).tiles[1][1]).toBeGreaterThan(top + 100);
      const [tileX, tileY] = (await layout()).tiles[0];
      await page.mouse.click(Math.max(32, tileX + 24), tileY + 60, { delay: 100 });
      await clickControl("Comment");
      await expect(scene).toHaveAttribute("data-diff-canvas-status", /pull request/i);
      await clickControl("Edit lines");
      await expect(scene).toHaveAttribute("data-diff-canvas-editor");
      await page.keyboard.type("// canvas draft", { delay: 50 });
      await expect.poll(() => page.evaluate(() =>
        Object.keys(localStorage)
          .filter(key => key.startsWith("repo-city-source-draft-v1:"))
          .map(key => JSON.parse(localStorage.getItem(key)))
      )).toEqual([expect.objectContaining({
        text: expect.stringContaining("// canvas draft"),
        identity: expect.objectContaining({ path: "old.rs", before: true, range: { start: 0, end: 1 } }),
      })]);
      await page.keyboard.press("Escape", { delay: 90 });
      await expect(scene).not.toHaveAttribute("data-diff-canvas-editor");
      await page.keyboard.press("ControlOrMeta+Minus", { delay: 120 });
      await expect.poll(async () => (await layout()).zoom).toBeLessThan(original.zoom);
    } else {
      const [x, y] = (await layout()).tiles[0];
      await page.mouse.click(x + 28, y + 62, { delay: 100 });
      await clickControl("Comment");
      await expect(scene).toHaveAttribute("data-diff-canvas-editor");
      await page.keyboard.type("Keep this deleted line in mind.", { delay: 40 });
      await clickControl("Save comment");
      await expect(scene).not.toHaveAttribute("data-diff-canvas-editor");
      await expect.poll(() => page.evaluate(() => JSON.parse(repoCity.reviewRead()).notes)).toEqual(
        [expect.objectContaining({
          text: "Keep this deleted line in mind.",
          anchor: { path: "old.rs", before: true, start: 1, end: 1 },
        })]
      );
    }
    await page.screenshot({ path: testInfo.outputPath("fullscreen-diff.png") });
    await page.keyboard.press("Escape", { delay: 120 });
    await expect(scene).not.toHaveAttribute("data-diff-canvas");
    await expectOpen(page, "Changed files");
  });
}
