import { test, expect } from "@playwright/test";

const after = "b".repeat(40);
const before = "a".repeat(40);

test("opens a public repository and fetches source after the WASM city starts", async ({ page }) => {
  const faults = [];
  page.on("pageerror", error => faults.push(error.message));
  await page.route("https://api.github.com/**", async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/demo") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: after, parents: [{ sha: before }] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: {
      tree: [{ path: "src/demo.rs", type: "blob", mode: "100644", sha: path.endsWith(after) ? after : before, size: 64 }],
    } });
    const source = `fn demo() {\n    println!("source ${path.endsWith(after) ? "after" : "before"}");\n}\n`;
    return route.fulfill({ json: { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/");
  await page.getByRole("textbox", { name: "Public GitHub repository" }).fill("owner/demo");
  const blob = page.waitForResponse(response => response.url().includes("/git/blobs/"), { timeout: 75_000 }).catch(() => null);
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.locator("#status")).toBeEmpty({ timeout: 70_000 });
  expect(await blob).not.toBeNull();
  await expect(page.locator("form")).toBeHidden({ timeout: 90_000 });
  await expect(page.locator("canvas")).toBeVisible();
  await expect.poll(async () => (await page.locator("canvas").screenshot()).length, { timeout: 30_000 }).toBeGreaterThan(15_000);
  expect(faults).toEqual([]);
});

test("reports missing repositories and keeps the form usable", async ({ page }) => {
  await page.route("https://api.github.com/**", route => route.fulfill({ status: 404 }));
  await page.goto("/");
  await page.getByRole("textbox", { name: "Public GitHub repository" }).fill("owner/missing");
  await page.getByRole("button", { name: "Open city" }).click();
  await expect(page.getByRole("status")).toHaveText("Public repository, revision, or file not found.");
  await expect(page.getByRole("button", { name: "Open city" })).toBeEnabled();
});
