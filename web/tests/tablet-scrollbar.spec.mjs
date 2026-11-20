import { test, expect } from "@playwright/test";
import { gpuClick, readTablet, servicePanel } from "./gpu-tablet.mjs";

async function openHelp(page) {
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    const source = "fn main() {}\n", sha = "b".repeat(40);
    return route.fulfill({ json: path === "/repos/owner/scroll" ? { default_branch: "main" }
      : path.includes("/commits/") ? { sha, parents: [] }
      : path.includes("/git/trees/") ? { tree: [{ path: "main.rs", type: "blob", mode: "100644", sha, size: source.length }] }
      : { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Fscroll");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.locator("canvas").focus();
  await page.keyboard.press("h", { delay: 150 });
  await expect.poll(async () => (await readTablet(page))?.title).toBe("Help");
}
const part = async (page, kind) => (await readTablet(page))?.layout.scrollbars?.find(item => item.kind === kind);
async function point(page, kind) {
  await expect.poll(async () => Boolean((await part(page, kind))?.bounds[3])).toBe(true);
  const { bounds } = await part(page, kind);
  return [bounds[0] + bounds[2] / 2, bounds[1] + bounds[3] / 2];
}

test("VibePad scroll arrows, thumb drag and track paging follow the visible content", async ({ page }, info) => {
  // Given help content taller than the tablet, show the themed scrollbar.
  await openHelp(page);
  await expect.poll(async () => (await part(page, "track"))?.range ?? 0).toBeGreaterThan(0);
  const fixedIds = await servicePanel(page).locator("header button, nav.vibepad-tabs button").evaluateAll(nodes => nodes.map(node => node.dataset.tabletControl));
  let stableLayout;
  let previousBounds, stableFrames = 0;
  await expect.poll(async () => {
    const { controls, scrollbars } = (await readTablet(page)).layout;
    const header = controls.filter(control => fixedIds.includes(control.id));
    const track = scrollbars?.find(item => item.kind === "track");
    if (header.length !== fixedIds.length || !track) return false;
    // Compare controls from the same frame after the tablet stops moving.
    const bounds = JSON.stringify([...header.map(control => control.bounds), track.bounds]);
    stableFrames = bounds === previousBounds ? stableFrames + 1 : 0;
    previousBounds = bounds;
    if (stableFrames < 3) return false;
    stableLayout = { header, track };
    return true;
  }).toBe(true);
  const { header: before, track: settledTrack } = stableLayout;
  const headerBottom = Math.max(...before.map(control => control.bounds[1] + control.bounds[3]));
  expect(settledTrack.bounds[1]).toBeGreaterThanOrEqual(headerBottom - 1);
  await page.screenshot({ path: info.outputPath("vibepad-scrollbar.png") });
  await page.mouse.click(...await point(page, "down"), { delay: 100 });
  await expect.poll(async () => (await part(page, "track"))?.offset ?? 0).toBeGreaterThan(0);
  for (const previous of before) {
    const current = (await readTablet(page)).layout.controls.find(control => control.id === previous.id);
    expect(current.bounds[1]).toBeCloseTo(previous.bounds[1], 0);
    expect(current.visible).toBe(true);
  }
  const afterArrow = (await part(page, "track")).offset;
  const thumb = await point(page, "thumb");
  await page.mouse.move(...thumb); await page.mouse.down();
  await page.mouse.move(thumb[0], thumb[1] + 90, { steps: 12 }); await page.mouse.up();
  await expect.poll(async () => (await part(page, "track"))?.offset ?? 0).toBeGreaterThan(afterArrow);
  const beforePage = (await part(page, "track")).offset;
  const track = (await part(page, "track")).bounds;
  await page.mouse.click(track[0] + track[2] / 2, track[1] + 3, { delay: 100 });
  await expect.poll(async () => (await part(page, "track"))?.offset ?? Infinity).toBeLessThan(beforePage);
  await page.screenshot({ path: info.outputPath("vibepad-scrollbar-scrolled.png") });
});

test("short tablet pages keep the gutter stable without scrollable content", async ({ page }) => {
  await openHelp(page);
  const width = (await part(page, "track")).bounds[2];
  await gpuClick(page, servicePanel(page).getByRole("button", { name: "Files", exact: true, includeHidden: true }));
  await expect.poll(async () => (await part(page, "track"))?.range).toBe(0);
  expect((await part(page, "track")).bounds[2]).toBeCloseTo(width, 0);
  await page.mouse.click(...await point(page, "down"), { delay: 100 });
  expect((await part(page, "track")).offset).toBe(0);
});

test.describe("touch scrolling", () => {
  test.use({ hasTouch: true });
  test("a swipe scrolls the page and dragging the thumb stays attached to the scrollbar", async ({ page }) => {
    await openHelp(page);
    await expect.poll(async () => (await part(page, "track"))?.range ?? 0).toBeGreaterThan(0);
    const input = await page.context().newCDPSession(page);
    async function drag(from, to) {
      await input.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: from[0], y: from[1] }] });
      for (let step = 1; step <= 10; step++) {
        await input.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: from[0] + (to[0] - from[0]) * step / 10, y: from[1] + (to[1] - from[1]) * step / 10 }] });
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
      }
      await input.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    }
    await drag([500, 500], [500, 430]);
    await expect.poll(async () => (await part(page, "track"))?.offset ?? 0).toBeGreaterThan(0);
    const beforeThumb = (await part(page, "track")).offset;
    const thumb = await point(page, "thumb");
    await drag(thumb, [thumb[0], thumb[1] + 50]);
    await expect.poll(async () => (await part(page, "track"))?.offset ?? 0).toBeGreaterThan(beforeThumb);
    await input.detach();
  });
});
