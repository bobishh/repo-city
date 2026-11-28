import { test, expect } from "@playwright/test";

test("switching to Abyss keeps an exact comparison, and cancelling keeps the current world", async ({ page }) => {
  const head = "b".repeat(40), base = "a".repeat(40), mergeBase = "c".repeat(40);
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route(/\/game\/(?:[^/]+\/)?repo_city\.js$/, route =>
    route.fulfill({ contentType: "text/javascript", body: "export default function start() {}" }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    const tree = [{ path: "station.rs", type: "blob", mode: "100644", sha: head, size: 24 }];
    const json = path.includes("/git/trees/") ? { tree } : path.includes("/compare/")
      ? { merge_base_commit: { sha: mergeBase } }
      : { sha: path.endsWith(base) ? base : head, parents: [] };
    return route.fulfill({ json });
  });
  await page.goto(`/?repo=owner%2Ftrench&ref=${head}&base=${base}&exact=1&world-picker=1`);
  const form = page.getByRole("form", { name: "Open repository" });
  const brand = page.locator("#logo");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 30_000 });
  await brand.click();
  await form.locator("select[name=world]").selectOption("abyss");
  await form.getByRole("button", { name: "Close repository picker" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-world", "code-city");
  await brand.click();
  await form.locator("select[name=world]").selectOption("abyss");
  await form.getByRole("button", { name: "Open Abyss" }).click();
  await expect(page).toHaveURL(/world=abyss/);
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 30_000 });
  await expect(page.locator("canvas")).toHaveAccessibleName("Abyss");
  expect(await page.evaluate(() => JSON.parse(window.repoCity.snapshot()).baseline)).toBe(base);
});

test("selects Abyss for the pinned repository and keeps the chooser usable after a load error", async ({ page }, testInfo) => {
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => route.fulfill({
    status: 404, json: { message: "Not Found" },
  }));
  await page.goto("/?world=abyss&world-picker=1&repo=owner%2Fdeep&ref=feature&base=main");
  const form = page.getByRole("form", { name: "Open repository" });
  await expect(page).toHaveTitle("Abyss · Repo City Web");
  await expect(form.locator("select[name=world]")).toHaveValue("abyss");
  await expect(form.locator("[name=ref]")).toHaveValue("feature");
  await expect(form.locator("[name=base]")).toHaveValue("main");
  await expect(form.getByRole("button", { name: "Open Abyss" })).toBeEnabled();
  await expect(form.getByRole("status")).toContainText(/not found|404/i);
  const cover = page.locator("#start-screen img");
  await expect(cover).toHaveAttribute("src", "./assets/abyss-cover.webp");
  await expect(cover).toBeVisible();
  await expect.poll(() => cover.evaluate(image => image.naturalWidth)).toBeGreaterThan(1200);
  await page.screenshot({ path: testInfo.outputPath("abyss-picker.png") });
  await form.locator("select[name=world]").selectOption("code-city");
  await expect(form.getByRole("button", { name: "Open city" })).toBeEnabled();
  await expect(page).toHaveTitle("Repo City Web");
});

test("mobile divers can drop a buoy and hold the exterior pilot view", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  try {
    const page = await context.newPage();
    await page.route("**/touch-fixture*", route => route.fulfill({ contentType: "text/html", body: `
      <!doctype html><meta name="viewport" content="width=device-width,initial-scale=1">
      <link rel="stylesheet" href="/style.css"><main><canvas></canvas></main>
      <script type="module">
        import { createTouchControls } from "/touch.mjs";
        const diving = () => location.hash === "#abyss";
        window.input = createTouchControls({ canvas: document.querySelector("canvas"), diving });
        document.documentElement.dataset.playing = "";
      </script>` }));
    await page.goto("/touch-fixture#abyss");
    const buoy = page.getByRole("button", { name: "Drop or retrieve a survey buoy" });
    const pilot = page.getByRole("button", { name: "Hold to see the pilot" });
    await buoy.tap();
    expect(await page.evaluate(() => JSON.parse(window.input.take()).pulses)).toEqual(["KeyO"]);
    const box = await pilot.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    expect(await page.evaluate(() => JSON.parse(window.input.take()).held)).toContain("KeyI");
    await page.mouse.up();
    expect(await page.evaluate(() => JSON.parse(window.input.take()).held)).not.toContain("KeyI");
    await page.goto("/touch-fixture?world=code-city");
    await expect(page.getByRole("button", { name: "Fly up" })).toBeVisible();
    await expect(buoy).toBeHidden();
    await expect(pilot).toBeHidden();
  } finally {
    await context.close();
  }
});
