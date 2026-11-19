import { test, expect } from "@playwright/test";

// Given the browser build, opening a repository and a failed request
// must use only the application and GitHub, without an event collector.
test("repository loading sends no analytics, including on failure", async ({ page, baseURL, request }) => {
  const origin = new URL(baseURL).origin;
  const unexpected = [];
  const faults = [];
  const sha = "b".repeat(40);
  const source = "fn demo() {}\n";
  page.on("pageerror", error => faults.push(error.message));
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  await page.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (url.origin === origin) {
      if (url.pathname === "/github/session") return route.fulfill({ json: { enabled: false } });
      return route.continue();
    }
    if (url.origin === "https://api.github.com") {
      if (url.pathname === "/repos/owner/missing") return route.fulfill({ status: 404 });
      if (url.pathname === "/repos/owner/demo") return route.fulfill({ json: { default_branch: "main" } });
      if (url.pathname.includes("/commits/")) return route.fulfill({ json: { sha, parents: [] } });
      if (url.pathname.includes("/git/trees/")) return route.fulfill({ json: {
        tree: [{ path: "src/demo.rs", type: "blob", mode: "100644", sha, size: source.length }],
      } });
      return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
    }
    unexpected.push(route.request().url());
    return route.abort();
  });

  await page.goto("/?repo=owner%2Fdemo");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await expect(page.locator("canvas")).toBeVisible();
  await expect.poll(() => page.evaluate(() => JSON.parse(repoCity.snapshot()).after["src/demo.rs"]?.id)).toBe(sha);

  await page.goto("/?repo=owner%2Fmissing");
  await expect(page.locator("#status")).toContainText("not found");
  await expect(page.getByRole("button", { name: "Open city" })).toBeEnabled();
  expect(faults).toEqual([]);
  expect(unexpected).toEqual([]);

  for (const asset of ["analytics.mjs", "analytics-config.mjs"]) {
    expect((await request.get(`/${asset}`)).status()).toBe(404);
  }
});
