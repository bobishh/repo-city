import { test, expect } from "@playwright/test";
import { expectOpen, expectClosed, gpuClick } from "./gpu-tablet.mjs";

const head = "b".repeat(40), base = "a".repeat(40);
const path = "src/frost.rs";

async function repository(page) {
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
    localStorage.setItem("repo-city-reading-v1", JSON.stringify({
      mode: "delayed", block_lines: 6, block_kind: "lines", auto_advance: false,
      min_seconds: 60, max_seconds: 60, show_all_source: false,
    }));
  });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: true, login: "reviewer" } }));
  await page.route(/\/github\/api\/|https:\/\/api.github.com\//, route => {
    const url = new URL(route.request().url()).pathname.replace("/github/api", "");
    const source = Array.from({ length: 30 }, (_, index) => [
      `fn frost_example_${index}() {`,
      `    let message = "${url.endsWith(base) ? "before" : "after"}: readable source has a visible shape";`,
      "    if !message.is_empty() {",
      "        println!(\"{}\", message);",
      "    }",
      "}",
    ][index % 6]).join("\n") + "\n";
    const comment = (id, start, end, body, extra = {}) => ({
      id, node_id: `comment-${id}`, path, start_line: start, line: end,
      side: "RIGHT", start_side: "RIGHT", body, user: { login: "ada" },
      html_url: `https://github.com/owner/frost/pull/8#discussion_r${id}`, ...extra,
    });
    return route.fulfill({ json: url.endsWith("/pulls/8") ? {
      state: "open", base: { sha: base }, head: { sha: head },
    } : url.endsWith("/comments") ? [
      comment(11, 7, 12, "Explain this entire six-line range."),
      comment(12, 19, 24, "This thread belongs to an older revision.", {
        path: "src/removed.rs", line: null, position: null, original_line: 24, original_start_line: 19,
        original_commit_id: "c".repeat(40),
      }),
    ] : url.includes("/compare/") ? { merge_base_commit: { sha: base } }
      : url.includes("/git/trees/") ? { tree: [{ path, type: "blob", mode: "100644", sha: url.endsWith(base) ? base : head, size: source.length }] }
      : url.includes("/git/blobs/") ? { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") }
      : url.includes("installations") ? { installations: [] } : [] });
  });
  await page.route("**/github/review/owner/frost/8", route => route.fulfill({ json: {
    headRefOid: head,
    reviewThreads: { pageInfo: { hasNextPage: false }, nodes: [
      { isResolved: false, comments: { nodes: [{ id: "comment-11" }] } },
      { isResolved: false, isOutdated: true, comments: { nodes: [{ id: "comment-12" }] } },
    ] },
  } }));
  await page.goto("/?pr=https%3A%2F%2Fgithub.com%2Fowner%2Ffrost%2Fpull%2F8");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect.poll(() => page.evaluate(() => JSON.parse(repoCity.reviewRead()).notes.length)).toBe(2);
}

async function screenshot(page, testInfo, name) {
  const png = await page.locator("canvas").screenshot({ path: testInfo.outputPath(name) });
  // Count the warm cue in the wall area, excluding HUD and the hand-held tool.
  return page.evaluate(async encoded => {
    const bytes = Uint8Array.from(atob(encoded), byte => byte.charCodeAt(0));
    const image = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const context = new OffscreenCanvas(image.width, image.height).getContext("2d");
    context.drawImage(image, 0, 0);
    const { data, width } = context.getImageData(0, 0, image.width, image.height);
    let warm = 0;
    for (let y = 160; y < 600; y++) for (let x = 160; x < 1050; x++) {
      const index = (y * width + x) * 4;
      if (data[index] > 125 && data[index] > data[index + 2] + 35 && data[index] > data[index + 1] * 1.1) warm++;
    }
    image.close();
    return warm;
  }, png.toString("base64"));
}

test("comment flight thaws its range with a fading cue without awarding reading credit", async ({ page }, testInfo) => {
  const faults = [];
  page.on("pageerror", error => faults.push(error.message));
  page.on("console", entry => {
    if (entry.type() === "error" && /shader|wgpu|webgpu|validation|panic/i.test(entry.text())) faults.push(entry.text());
  });
  await repository(page);
  const canvas = page.locator("canvas");
  await canvas.click();
  await expect(canvas).toHaveAttribute("data-pointer-locked", "");
  await page.keyboard.press("KeyE", { delay: 180 });
  await page.waitForTimeout(1800);
  await screenshot(page, testInfo, "unread-frost.png");
  await expect(page.locator("#progress-summary")).toContainText("0 lines revealed");

  await page.keyboard.press("KeyJ", { delay: 180 });
  await expectOpen(page);
  const panel = page.locator("#review-dialog");
  // J opens the aimed comment directly; away from a mark it opens the list.
  const current = panel.getByRole("button", { name: /src\/frost\.rs.*GitHub thread/, includeHidden: true });
  if (await current.count()) await gpuClick(page, current);
  await expect(panel).toContainText("Explain this entire six-line range.");
  await gpuClick(page, panel.getByRole("button", { name: "Fly to line", exact: true, includeHidden: true }));
  await expectClosed(page);
  // Existing wall flights finish within the same 1.8-second allowance used above.
  await page.waitForTimeout(1800);
  const arrival = await screenshot(page, testInfo, "guided-arrival.png");
  await expect(page.locator("#progress-summary")).toContainText("0 lines revealed");
  await page.waitForTimeout(4000);
  const settled = await screenshot(page, testInfo, "guided-settled.png");
  expect(arrival, "arrival displays a warm range cue that then fades").toBeGreaterThan(settled + 50);
  await expect(page.locator("#progress-summary")).toContainText("0 lines revealed");

  await canvas.evaluate(element => element.requestPointerLock());
  await expect(canvas).toHaveAttribute("data-pointer-locked", "");
  await canvas.focus();
  await page.keyboard.press("KeyJ", { delay: 180 });
  await expectOpen(page);
  const back = panel.getByRole("button", { name: "Back", exact: true, includeHidden: true });
  if (await back.count()) await gpuClick(page, back);
  await gpuClick(page, panel.getByRole("button", { name: /src\/removed\.rs.*Original location/, includeHidden: true }));
  await expect(panel).toContainText("no current line anchor");
  await expect(panel.getByRole("button", { name: "Fly to line", exact: true, includeHidden: true })).toHaveCount(0);
  expect(faults).toEqual([]);
});
