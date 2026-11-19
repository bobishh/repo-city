import { test, expect } from "@playwright/test";
import { gpuClick, gpuFill, expectOpen, expectClosed } from "./gpu-tablet.mjs";

const head = "b".repeat(40), base = "a".repeat(40);
for (const pull of [false, true]) test(`pasted trace opens a route without write access (${pull ? "PR" : "repository"})`, async ({ page }, info) => {
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  const source = "def run\n  puts :example\nend\n".repeat(20);
  const secondBlob = "c".repeat(40);
  let secondRequested = false;
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", async route => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/git/blobs/" + secondBlob)) {
      secondRequested = true;
      await new Promise(resolve => setTimeout(resolve, 700));
    }
    return route.fulfill({ json: path === "/repos/owner/trace" ? { default_branch: "main" }
      : path.endsWith("/pulls/8") ? { state: "closed", merged: true, base: { sha: base }, head: { sha: head } }
      : path.includes("/compare/") ? { merge_base_commit: { sha: base } }
      : path.includes("/commits/") ? { sha: head, parents: [] }
      : path.includes("/git/trees/") ? { tree: ["lib/first.rb", "lib/second.rb"].map((path, index) => ({ path, type: "blob", mode: "100644", sha: index ? secondBlob : head, size: source.length })) }
      : path.includes("/git/blobs/") ? { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } : [] });
  });
  await page.goto(pull ? "/?pr=https%3A%2F%2Fgithub.com%2Fowner%2Ftrace%2Fpull%2F8" : "/?repo=owner%2Ftrace");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.locator("canvas").focus();
  await page.keyboard.press("Tab", { delay: 120 });
  const pad = page.locator("#review-dialog");
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Trace", exact: true }));
  await expectOpen(page, "Paste a trace");
  const input = pad.getByRole("textbox", { includeHidden: true, name: "Stack trace" });
  await gpuFill(page, input, "vendor/not-here.rb:20");
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Build route", exact: true }));
  await expect(pad).toContainText("No matching frames");
  await expect(input).toHaveValue("vendor/not-here.rb:20");
  await expect(pad.getByRole("button", { includeHidden: true, name: "Follow trace", exact: true })).toBeDisabled();
  const trace = "RuntimeError: example\nlib/first.rb:2:in `run'\n/project/lib/second.rb:17:in `call'\nvendor/not-here.rb:20\n";
  await gpuFill(page, input, trace);
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Build route", exact: true }));
  await expect(pad).toContainText("2 frames");
  await expect(pad).toContainText("lib/first.rb:2");
  await expect(pad).toContainText("lib/second.rb:17");
  await expect(pad).toContainText("not-here.rb");
  await page.screenshot({ path: info.outputPath("trace-preview.png") });
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Follow trace", exact: true }));
  await expectClosed(page);
  await page.waitForTimeout(3500);
  expect(secondRequested).toBe(true);
  await page.keyboard.press("KeyW", { delay: 150 });
  await page.waitForTimeout(3500);
  await page.screenshot({ path: info.outputPath("trace-second-frame.png") });
  await page.keyboard.press("Escape", { delay: 150 });
  await page.keyboard.press("Home", { delay: 150 });
  await page.waitForTimeout(3500);
  await page.keyboard.press("KeyB", { delay: 150 });
  await expect(page.locator("#blame")).toHaveAttribute("href", /lib\/second\.rb#/);
});
