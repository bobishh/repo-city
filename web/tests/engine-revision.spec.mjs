import { expect, test } from "@playwright/test";

test("opens a city with paired versioned engine files even when the previous engine is cached", async ({ page }) => {
  const revision = "a".repeat(40);
  const source = "fn example() {}\n";
  const assets = [];
  await page.route("**/game/repo_city.js", route =>
    route.fulfill({ contentType: "text/javascript", body: 'export default () => { throw Error("Previous engine"); };' }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/engine-test") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: revision, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({
      json: { tree: [{ path: "example.rs", type: "blob", size: source.length, sha: revision, mode: "100644" }] },
    });
    return route.fulfill({ json: { content: Buffer.from(source).toString("base64"), encoding: "base64", size: source.length } });
  });
  page.on("response", response => {
    if (!response.ok()) return;
    const path = new URL(response.url()).pathname;
    if (path.startsWith("/game/")) assets.push(path);
  });
  await page.goto("/?repo=owner%2Fengine-test");
  await expect.poll(async () => page.evaluate(() => {
    const status = document.querySelector("#status")?.textContent;
    return document.querySelector("#start-screen")?.hidden ? "open" :
      status?.startsWith("Loading") || !status ? "loading" : status;
  }), { timeout: 20_000 }).toBe("open");

  const glue = assets.find(path => /^\/game\/[0-9a-f]{24}\/repo_city\.js$/.test(path));
  expect(glue).toBeTruthy();
  expect(assets).toContain(glue.replace(/repo_city\.js$/, "repo_city_bg.wasm"));
  await expect(page.locator("#city-file")).toHaveText("example.rs");
});
