import { test, expect } from "@playwright/test";

const head = "b".repeat(40), base = "a".repeat(40);
for (const history of [true, false]) test(`long Elm code keeps canonical lines while scrolling blame and diff (${history ? "history loaded" : "history unavailable"})`, async ({ page }, info) => {
  await page.addInitScript(() => {
    let locked = null;
    Object.defineProperty(document, "pointerLockElement", { get: () => locked });
    HTMLCanvasElement.prototype.requestPointerLock = function () { locked = this; queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange"))); return Promise.resolve(); };
    document.exitPointerLock = () => { locked = null; queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange"))); };
    localStorage.setItem("repo-city-guide-v1", "seen");
    localStorage.setItem("repo-city-reading-v1", JSON.stringify({ mode: "immediate", block_lines: 6, block_kind: "fixed", auto_advance: false, min_seconds: null, max_seconds: null }));
  });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: true, login: "reviewer" } }));
  await page.route(/\/github\/api\/|https:\/\/api.github.com\//, route => {
    const path = new URL(route.request().url()).pathname.replace("/github/api", "");
    const source = `module JobGame.Campaign exposing (${"Model, Outcome(..), Stage(..), charge, daysUntilDeadline, ".repeat(180)}${path.endsWith(base) ? "BEFORE_TAIL" : "ELM_TAIL"})\n\ntype Outcome = InProgress | Passed | Failed\n`;
    return route.fulfill({ json: path.endsWith("/pulls/8") ? { state: "closed", merged: true, base: { sha: base }, head: { sha: head } }
      : path.includes("/compare/") ? { merge_base_commit: { sha: base } }
      : path.includes("/git/trees/") ? { tree: [{ path: "src/JobGame/Campaign.elm", type: "blob", mode: "100644", sha: path.endsWith(base) ? base : head, size: source.length }] }
      : path.includes("/git/blobs/") ? { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") }
      : path.includes("installations") ? { installations: [] } : [] });
  });
  await page.route("**/github/review/owner/wrap/8", route => route.fulfill({ json: { headRefOid: head, reviewThreads: { pageInfo: { hasNextPage: false }, nodes: [] } } }));
  await page.route(/\/github\/blame\?/, route => history
    ? route.fulfill({ json: { ranges: [{ startingLine: 1, endingLine: 3, commit: { abbreviatedOid: "b0123456", committedDate: "2026-11-19T08:00:00Z", author: { name: "Ada" } } }] } })
    : route.fulfill({ status: 503, json: { error: "History unavailable" } }));
  await page.goto("/?pr=https%3A%2F%2Fgithub.com%2Fowner%2Fwrap%2Fpull%2F8");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.locator("canvas").click();
  await page.keyboard.press("KeyE", { delay: 180 });
  await page.waitForTimeout(1800);
  await page.keyboard.press("KeyB", { delay: 180 });
  await page.waitForTimeout(900);
  await page.keyboard.press("PageDown", { delay: 180 });
  await page.keyboard.press("PageDown", { delay: 180 });
  await expect(page.locator("#blame")).toHaveAttribute("href", /#L1$/);
  await page.screenshot({ path: info.outputPath("wrapped-elm-blame.png") });
  await page.keyboard.press("KeyG", { delay: 180 });
  await page.waitForTimeout(900);
  await page.keyboard.press("PageDown", { delay: 180 });
  await page.keyboard.press("Digit1", { delay: 180 });
  await page.mouse.down(); await page.waitForTimeout(200); await page.mouse.up();
  await expect.poll(() => page.evaluate(() => JSON.parse(repoCity.reviewRead()).notes.filter(note => !note.remote).length)).toBe(1);
  const note = await page.evaluate(() => JSON.parse(repoCity.reviewRead()).notes.find(note => !note.remote));
  expect(note.anchor).toMatchObject({ path: "src/JobGame/Campaign.elm", start: 1, end: 1 });
  await page.screenshot({ path: info.outputPath("wrapped-elm-diff.png") });
});
