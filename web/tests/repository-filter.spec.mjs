import { test, expect } from "@playwright/test";

// A normal repository URL compares its latest commit with the parent. Its
// changes filter must work in project overview as well as in a pull request.
test("a repository's last-commit comparison filters from project overview", async ({ page }, info) => {
  const head = "b".repeat(40), base = "a".repeat(40), stable = "c".repeat(40);
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    return route.fulfill({ json: path === "/repos/owner/filter" ? { default_branch: "main" }
      : path.includes("/commits/") ? { sha: head, parents: [{ sha: base }] }
      : path.includes("/git/trees/") ? { tree: [
        { path: "changed.rs", type: "blob", mode: "100644", sha: path.endsWith(base) ? base : head, size: 80 },
        { path: "same.rs", type: "blob", mode: "100644", sha: stable, size: 80 },
      ] }
      : path.includes("/git/blobs/") ? { encoding: "base64", size: 80, content: Buffer.from("fn example() {}\n".repeat(5)).toString("base64") } : [] });
  });
  await page.goto("/?repo=owner%2Ffilter");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas");
  await canvas.focus();
  await expect.poll(() => page.evaluate(() => globalThis.repoCity?.canFilterChanges())).toBe(true);
  await expect.poll(() => page.evaluate(() => globalThis.repoCity?.comparisonLabel())).toBe("Last commit");
  await page.keyboard.press("KeyM", { delay: 150 });
  await page.keyboard.press("KeyM", { delay: 150 });
  await expect(canvas).toHaveAttribute("data-overview");
  await page.keyboard.press("KeyF", { delay: 150 });
  await expect(canvas).toHaveAttribute("data-changes-only", "");
  await page.waitForTimeout(900);
  await page.screenshot({ path: info.outputPath("last-commit-filter.png") });
  await page.keyboard.press("KeyF", { delay: 150 });
  await expect(canvas).not.toHaveAttribute("data-changes-only");
});
