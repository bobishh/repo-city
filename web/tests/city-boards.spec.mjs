import { readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";

test("the city document board loads the README and its image from the opened revision", async ({ page }) => {
  const revision = "a".repeat(40);
  const readme = "b".repeat(40);
  const picture = "c".repeat(40);
  const code = "d".repeat(40);
  const markdown = Buffer.from("# City board\n\n![Robot](assets/robot.png)\n\n```mermaid\nflowchart LR\nREADME --> Board\n```");
  const image = await readFile(new URL("../assets/apple-touch-icon.png", import.meta.url));
  const bytes = new Map([[readme, markdown], [picture, image], [code, Buffer.from("fn main() {}\n")]]);
  const files = [["README.md", readme], ["assets/robot.png", picture], ["example.rs", code]];
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/repos/owner/board-test") return route.fulfill({ json: { default_branch: "main" } });
    if (path.includes("/commits/")) return route.fulfill({ json: { sha: revision, parents: [] } });
    if (path.includes("/git/trees/")) return route.fulfill({ json: { tree: files.map(([path, sha]) => ({
      path, sha, size: bytes.get(sha).length, type: "blob", mode: "100644",
    })) } });
    const content = bytes.get(path.split("/").at(-1));
    return content ? route.fulfill({ json: { content: content.toString("base64"), encoding: "base64", size: content.length } })
      : route.fulfill({ status: 404, json: {} });
  });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const imageRequest = page.waitForResponse(response => response.url().endsWith(`/git/blobs/${picture}`), { timeout: 45_000 });
  await page.goto("/?repo=owner%2Fboard-test");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 45_000 });
  expect((await imageRequest).ok()).toBe(true);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect(errors).toEqual([]);
});
