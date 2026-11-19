import { test, expect } from "@playwright/test";
import { gpuClick, gpuFill, gpuSelect, expectOpen, expectClosed } from "./gpu-tablet.mjs";
const head = "b".repeat(40);
const otelSpan = (id, parent, path, line, traceId = "abc") => ({ traceId, spanId: id, parentSpanId: parent, name: id, startTimeUnixNano: id === "root" ? "1700000000000000000" : "1700000000001000000", endTimeUnixNano: "1700000000002000000", attributes: [{ key: "code.file.path", value: { stringValue: path } }, { key: "code.line.number", value: { intValue: String(line) } }] });
for (const format of ["OpenTelemetry", "Chrome Trace"]) test(`${format} JSON builds a source span route in VibePad`, async ({ page }, info) => {
  await page.addInitScript(() => localStorage.setItem("repo-city-guide-v1", "seen"));
  const source = "def run\n  puts :example\nend\n".repeat(20);
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const path = new URL(route.request().url()).pathname;
    return route.fulfill({ json: path === "/repos/owner/spans" ? { default_branch: "main" }
      : path.includes("/commits/") ? { sha: head, parents: [] }
      : path.includes("/git/trees/") ? { tree: ["lib/first.rb", "lib/second.rb"].map(path => ({ path, type: "blob", mode: "100644", sha: head, size: source.length })) }
      : path.includes("/git/blobs/") ? { size: source.length, encoding: "base64", content: Buffer.from(source).toString("base64") } : [] });
  });
  await page.goto("/?repo=owner%2Fspans");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.locator("canvas").focus(); await page.keyboard.press("Tab", { delay: 120 });
  const pad = page.locator("#review-dialog");
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Trace", exact: true }));
  await expectOpen(page, "Paste a trace");
  const input = pad.getByRole("textbox", { includeHidden: true, name: "Stack trace" });
  await gpuFill(page, input, "{broken");
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Build route", exact: true }));
  await expect(pad).toContainText("Invalid trace JSON");
  await expect(input).toHaveValue("{broken");
  const json = format === "OpenTelemetry" ? { resourceSpans: [{ scopeSpans: [{ spans: [otelSpan("child", "root", "lib/second.rb", 17), otelSpan("root", "", "lib/first.rb", 2), otelSpan("other", "", "lib/first.rb", 5, "def")] }] }] }
    : { traceEvents: [{ ph: "E", pid: 2, tid: 3, ts: 2000 }, { ph: "B", name: "root", pid: 2, tid: 3, ts: 0, args: { file: "lib/first.rb", line: 2 } }, { ph: "X", name: "child", pid: 2, tid: 3, ts: 1000, dur: 500, args: { file: "lib/second.rb", line: 17 } }] };
  await gpuFill(page, input, JSON.stringify(json));
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Build route", exact: true }));
  await expect(pad).toContainText(format);
  if (format === "OpenTelemetry") {
    await expect(pad.getByRole("button", { includeHidden: true, name: "Follow trace", exact: true })).toBeDisabled();
    await gpuSelect(page, pad.getByLabel("Trace or thread"), "otel:abc");
  }
  await expect(pad).toContainText("lib/second.rb:17");
  await expect(pad).toContainText("child");
  await page.screenshot({ path: info.outputPath("json-span-preview.png") });
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Follow trace", exact: true }));
  await expectClosed(page); await page.waitForTimeout(3500);
  await page.keyboard.press("KeyW", { delay: 150 }); await page.waitForTimeout(3500);
  await page.screenshot({ path: info.outputPath("json-second-span.png") });
  await page.keyboard.press("Escape", { delay: 150 }); await page.keyboard.press("Home", { delay: 150 }); await page.waitForTimeout(3500);
  await page.keyboard.press("KeyB", { delay: 150 }); await expect(page.locator("#blame")).toHaveAttribute("href", /lib\/second\.rb#/);
});
