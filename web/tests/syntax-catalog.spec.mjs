import { test, expect } from "@playwright/test";

const source = '/- λ and 🪐 stay inside UTF-8 ranges. -/\nimport Mathlib\ntheorem city : 1 + 1 = 2 := by decide\n';

async function openLean(page, unavailable = false) {
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  if (unavailable) await page.route("**/syntax-chunks/source.lean4-*.mjs", route => route.abort());
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    const sha = "b".repeat(40), size = Buffer.byteLength(source);
    return route.fulfill({ json: path === "/repos/owner/lean" ? { default_branch: "main" }
      : path.includes("/commits/") ? { sha, parents: [] }
      : path.includes("/git/trees/") ? { tree: [{ path: "Mathlib/Basic.lean", type: "blob", mode: "100644", sha, size }] }
      : { size, encoding: "base64", content: Buffer.from(source).toString("base64") } });
  });
  await page.goto("/?repo=owner%2Flean");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.locator("canvas").click();
  await page.keyboard.press("KeyE", { delay: 180 });
  await page.keyboard.press("KeyK", { delay: 180 });
}

test("Lean source loads its grammar on demand without downloading unrelated languages", async ({ page }, info) => {
  const requests = [], faults = [];
  page.on("request", request => requests.push(request.url()));
  page.on("pageerror", error => faults.push(error.message));
  await openLean(page);
  await expect.poll(() => requests.some(url => /syntax-chunks\/source\.lean4-/.test(url)), { timeout: 30_000 }).toBe(true);
  await expect.poll(() => page.evaluate(source => [...repoCity.syntaxSpans("Mathlib/Basic.lean", source)].length, source)).toBeGreaterThan(0);
  const spans = await page.evaluate(source => [...repoCity.syntaxSpans("Mathlib/Basic.lean", source)], source);
  const bytes = Buffer.from(source);
  const tokens = [];
  for (let i = 0; i < spans.length; i += 3) tokens.push([bytes.subarray(spans[i], spans[i + 1]).toString(), spans[i + 2]]);
  expect(tokens.some(([text, kind]) => text === "theorem" && kind === 1)).toBe(true);
  expect(requests.some(url => /syntax-chunks\/source\.python-/.test(url))).toBe(false);
  expect(requests.some(url => url.includes("esm.sh"))).toBe(false);
  expect(faults).toEqual([]);
  await page.screenshot({ path: info.outputPath("lean-grammar.png") });
});

test("an unavailable grammar leaves the city responsive with plain source", async ({ page }) => {
  const faults = []; page.on("pageerror", error => faults.push(error.message));
  await openLean(page, true);
  await expect.poll(() => page.evaluate(() => repoCity.syntaxVersion()), { timeout: 30_000 }).toBeGreaterThan(0);
  expect(await page.evaluate(source => [...repoCity.syntaxSpans("Mathlib/Basic.lean", source)], source)).toEqual([]);
  await page.keyboard.press("h", { delay: 180 });
  await expect.poll(() => page.evaluate(() => repoCity.readTablet()?.title)).toBe("Help");
  expect(faults).toEqual([]);
});
