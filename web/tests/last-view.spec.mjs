import { test, expect } from "@playwright/test";
import { expectOpen, gpuClick } from "./gpu-tablet.mjs";

test("reopening a comparison returns to the last viewed file and wall", async ({ page }) => {
  const revision = "a".repeat(40);
  const source = "fn example() {}\n".repeat(40);
  const path = "src/zeta.rs";
  const key = "repo-city-last-view-v1:" + JSON.stringify(["owner/resume", revision, null]);
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
    localStorage.setItem("repo-city-guide-v1", "seen");
  });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const api = new URL(route.request().url()).pathname;
    return route.fulfill({ json: api === "/repos/owner/resume" ? { default_branch: "main" }
      : api.includes("/commits/") ? { sha: revision, parents: [] }
      : api.includes("/git/trees/") ? { tree: ["src/alpha.rs", path].map((path, index) => ({
        path, type: "blob", mode: "100644", sha: String(index + 1).repeat(40), size: source.length,
      })) }
      : { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Fresume");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const hud = page.locator("#city-file");
  await expect(hud).toHaveText("alpha.rs");
  await page.keyboard.press("Control+p", { delay: 120 });
  await expectOpen(page, "Find a file");
  await gpuClick(page, page.getByRole("button", { name: path, exact: true, includeHidden: true }));
  const canvas = page.locator("canvas");
  await canvas.evaluate(element => element.requestPointerLock());
  const saved = () => page.evaluate(key => JSON.parse(localStorage.getItem(key)), key);
  await expect.poll(async () => (await saved())?.path).toBe(path);
  await page.keyboard.press("KeyB", { delay: 150 });
  const blame = page.locator("#blame");
  await expect(blame).toBeVisible();
  await expect.poll(async () => Math.abs((await saved()).rotation[1])).toBeGreaterThan(0.5);

  await page.reload();
  await expect(hud).toHaveText("zeta.rs", { timeout: 90_000 });
  await expect(blame).toBeVisible();

  await page.evaluate(key => localStorage.setItem(key, "{broken"), key);
  await page.reload();
  await expect(hud).toHaveText("alpha.rs", { timeout: 90_000 });

  await expect(page.locator("#start-screen")).toBeHidden();
  await canvas.evaluate(element => element.requestPointerLock());
  await page.keyboard.press("KeyO", { delay: 150 });
  await expect(page.locator("#city-hint")).toHaveText("Walking · O or Space to fly");
  await page.keyboard.press("Space", { delay: 150 });
  await expect(page.locator("#city-hint")).toBeHidden();
});
