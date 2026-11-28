import { test, expect } from "@playwright/test";
import { gpuClick, expectOpen } from "./gpu-tablet.mjs";

const head = "b".repeat(40), base = "a".repeat(40);
for (const merged of [false, true]) test(`${merged ? "merged" : "closed"} PR accepts a painted comment and preserves it when publication is denied`, async ({ page }) => {
  await page.addInitScript(() => {
    // Match the existing city tests: headless Chromium lacks reliable OS pointer lock.
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
    localStorage.setItem("repo-city-reading-v1", JSON.stringify({ enabled: true, mode: "immediate", block_lines: 6, block_kind: "fixed", auto_advance: false, min_seconds: null, max_seconds: null }));
  });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: true, login: "reviewer" } }));
  await page.route(/\/github\/api\/|https:\/\/api.github.com\//, route => {
    const path = new URL(route.request().url()).pathname.replace("/github/api", "");
    const source = Array.from({ length: 32 }, (_, index) =>
      `fn inspect_historical_review_case_${index}_with_visible_source() { /* ${path.endsWith(base) ? "before" : "after"} */ }`,
    ).join("\n") + "\n";
    return route.fulfill({ json: path.endsWith("/pulls/8") ? { state: "closed", merged, base: { sha: base }, head: { sha: head } }
      : path.includes("/compare/") ? { merge_base_commit: { sha: base } }
      : path.includes("/git/trees/") ? { tree: [{ path: "src/demo.rs", type: "blob", mode: "100644", sha: path.endsWith(base) ? base : head, size: source.length }] }
      : path.includes("/git/blobs/") ? { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") }
      : path.includes("installations") ? { installations: [] } : [] });
  });
  const posts = [];
  await page.route("**/github/review/owner/demo/8", route => {
    if (route.request().method() === "GET") return route.fulfill({ json: { headRefOid: head, reviewThreads: { pageInfo: { hasNextPage: false }, nodes: [] } } });
    posts.push(route.request().postDataJSON());
    if (posts.length === 1) return route.fulfill({ status: 403, json: { error: "Publishing needs Pull requests: write." } });
    return route.fulfill({ status: 201, json: { comment: { html_url: "https://github.com/owner/demo/pull/8#discussion_r42" } } });
  });
  await page.goto("/?pr=https%3A%2F%2Fgithub.com%2Fowner%2Fdemo%2Fpull%2F8");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  expect(await page.evaluate(() => repoCity.reviewCanPaint())).toBe(true);
  await page.locator("canvas").click();
  await expect(page.locator("canvas")).toHaveAttribute("data-pointer-locked", "");
  await page.keyboard.press("KeyE", { delay: 180 });
  // Complete the existing flight animation before starting a tool at the wall.
  await page.waitForTimeout(1800);
  // Review the already readable diff; this scenario concerns PR publication,
  // independently of source washing and which reading block the camera frames.
  await page.keyboard.press("KeyG", { delay: 180 });
  await page.waitForTimeout(600);
  await page.keyboard.press("Digit1", { delay: 180 });
  await page.mouse.down();
  await page.waitForTimeout(200);
  await page.mouse.up();
  await expect.poll(() => page.evaluate(() => JSON.parse(repoCity.reviewRead()).notes.filter(note => !note.remote).length)).toBe(1);
  await page.keyboard.press("KeyJ", { delay: 180 });
  await expectOpen(page);
  const review = page.locator("#review-dialog");
  const draft = review.getByRole("button", { name: /Draft/, includeHidden: true });
  if (await draft.count()) await gpuClick(page, draft);
  const publish = review.getByRole("button", { name: "Publish to GitHub", exact: true, includeHidden: true });
  await gpuClick(page, publish);
  await expect(review.getByRole("status", { includeHidden: true })).toContainText("Pull requests: write");
  expect(await page.evaluate(() => JSON.parse(repoCity.reviewRead()).notes.some(note => !note.remote && !note.published))).toBe(true);
  await gpuClick(page, publish);
  await expect(review.getByRole("link", { name: "Published on GitHub", includeHidden: true })).toBeAttached();
  expect(posts).toHaveLength(2);
  expect(posts[1]).toMatchObject({ id: posts[0].id, revision: head, path: "src/demo.rs" });
});
