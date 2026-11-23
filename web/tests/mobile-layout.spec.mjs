import { test, expect } from "@playwright/test";
import { expectOpen, readTablet } from "./gpu-tablet.mjs";

test("mobile city and VibePad fit between Safari safe-area edges in both orientations", async ({ browser, baseURL }, info) => {
  const portrait = { width: 320, height: 568 };
  const landscape = { width: 734, height: 256 };
  const context = await browser.newContext({ baseURL, viewport: portrait, isMobile: true, hasTouch: true });
  const download = Promise.withResolvers();
  const engineRequested = Promise.withResolvers();
  try {
    const page = await context.newPage();
    const head = "b".repeat(40), base = "a".repeat(40);
    const file = "an_extraordinarily_long_filename_for_a_mobile_screen.rs";
    const body = "fn main() {}\n";
    await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
    await page.route("**/game/**/repo_city_bg.wasm", async route => {
      engineRequested.resolve();
      await download.promise;
      await route.continue();
    });
    await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
    await page.route("https://api.github.com/**", route => {
      const path = new URL(route.request().url()).pathname;
      return route.fulfill({ json: path === "/repos/owner/a-very-long-repository-name" ? { default_branch: "main" }
        : path.includes("/commits/") ? { sha: head, parents: [{ sha: base }] }
        : path.includes("/git/trees/") ? {
          tree: [{ path: file, type: "blob", mode: "100644", sha: path.endsWith(base) ? base : head, size: body.length }],
        } : { size: body.length, encoding: "base64", content: Buffer.from(body).toString("base64") } });
    });
    await page.goto("/?repo=owner%2Fa-very-long-repository-name");
    const pad = page.getByRole("button", { name: "Open VibePad" });
    await engineRequested.promise;
    await expect(page.locator("#repo-city")).toBeVisible();
    await expect(page.getByRole("form", { name: "Open repository" })).toBeVisible();
    await expect(page.locator("#touch-controls")).toBeHidden();
    download.resolve();
    await expect(pad).toBeVisible({ timeout: 90_000 });
    await expect(page.locator("#start-controls")).toBeHidden();
    await page.getByRole("button", { name: "Repo City", exact: true }).tap();
    await expect(page.locator("#repository-dialog")).toBeVisible();
    await expect(pad).toBeHidden();
    await page.getByRole("button", { name: "Close repository picker" }).tap();
    await expect(pad).toBeVisible();
    await expect(page.locator("#repository-query")).toHaveCSS("font-size", "16px");
    await expect(page.getByRole("group", { name: "Files visited" })).toContainText("/1");
    await page.evaluate(file => globalThis.repoCity.showBlame(file, false, 1), file);
    await expect(page.getByRole("link", { name: "Blame on GitHub" })).toBeVisible();

    async function checkLayout(insets) {
      const problems = await page.evaluate(insets => {
        const viewport = window.visualViewport;
        const areas = [...document.querySelectorAll(
          "#logo, #help, #blame, #city-progress, #touch-controls button, #repo-city",
        )].filter(element => element.getClientRects().length);
        const broken = [];
        for (const area of areas) {
          const box = area.getBoundingClientRect();
          if (box.left < insets.left || box.top < insets.top ||
              box.right > viewport.width - insets.right + 1 || box.bottom > viewport.height - insets.bottom + 1) {
            broken.push(area.id || area.title);
          }
          if (area.matches("button, a")) {
            const hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
            if (!area.contains(hit)) broken.push(`${area.id || area.title} covered`);
          }
        }
        const header = document.querySelector("#city-header").getBoundingClientRect();
        const firstControl = Math.min(...areas.filter(area => area.closest("#touch-controls"))
          .map(area => area.getBoundingClientRect().top));
        if (header.bottom + 8 > firstControl) broken.push("header overlaps flight controls");
        if (viewport.scale !== 1 || document.documentElement.scrollWidth > viewport.width + 1) broken.push("page overflow");
        return broken;
      }, insets);
      expect(problems).toEqual([]);
    }

    const noInsets = { left: 0, right: 0, top: 0, bottom: 0 };
    await checkLayout(noInsets);
    await page.screenshot({ path: info.outputPath("mobile-portrait.png") });
    await pad.tap();
    await expectOpen(page);
    await expect(pad).toBeHidden();
    const insets = { left: 47, right: 47, top: 0, bottom: 21 };
    await page.setViewportSize(landscape);
    await page.evaluate(insets => {
      for (const [side, size] of Object.entries(insets)) {
        document.documentElement.style.setProperty(`--safe-${side}`, `${size}px`);
      }
    }, insets);
    await expect.poll(async () => (await readTablet(page))?.layout.viewport)
      .toEqual([landscape.width - insets.left - insets.right, landscape.height - insets.bottom]);
    await expectOpen(page);
    await page.keyboard.press("Escape", { delay: 150 });
    await expect(pad).toBeVisible();
    await checkLayout(insets);

    await page.getByRole("button", { name: "How to use Repo City" }).tap();
    await expectOpen(page, "Help");
    let latest, previous = "", stable = 0;
    await expect.poll(async () => {
      const { controls, scrollbars } = (await readTablet(page)).layout;
      const heading = controls.find(item => item.kind === "heading" && item.text === "Help");
      const track = scrollbars.find(item => item.kind === "track" && item.range > 0);
      const down = scrollbars.find(item => item.kind === "down" && item.range > 0);
      if (!heading || !track || !down) return false;
      latest = { heading, track, down };
      const bounds = JSON.stringify(latest);
      stable = previous === bounds ? stable + 1 : 0;
      previous = bounds;
      return stable >= 3;
    }).toBe(true);
    const { heading, track, down } = latest;
    expect(heading.bounds[3]).toBeLessThan(21);
    expect(track.bounds[2]).toBeLessThan(15);
    await page.screenshot({ path: info.outputPath("mobile-landscape-pad.png") });

    const [x, y, width, height] = down.bounds;
    await page.touchscreen.tap(insets.left + x + width / 2, insets.top + y + height / 2);
    await expect.poll(async () => (await readTablet(page)).layout.scrollbars.find(item => item.kind === "track").offset)
      .toBeGreaterThan(0);
  } finally {
    download.resolve();
    await context.close();
  }
});
