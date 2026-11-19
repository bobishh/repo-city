import { test, expect } from "@playwright/test";
import { gpuClick, expectOpen, expectClosed, readTablet } from "./gpu-tablet.mjs";

const head = "b".repeat(40);
const path = "lib/example.rb";
const source = "def example\n  puts :hello\nend\n" + "\n".repeat(12) + "def unseen_tail\n  puts :leave_unchanged\nend\n";

async function fixture(page, binary = false) {
  await page.addInitScript(() => {
    localStorage.setItem("repo-city-guide-v1", "seen");
    let locked = null;
    Object.defineProperty(document, "pointerLockElement", { get: () => locked });
    HTMLCanvasElement.prototype.requestPointerLock = function () {
      locked = this;
      queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange")));
      return Promise.resolve();
    };
    document.exitPointerLock = () => {
      locked = null;
      queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange")));
    };
  });
  const bytes = binary ? Buffer.from([0, 1, 2]) : Buffer.from(source);
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    expect(route.request().method()).toBe("GET");
    const pathname = new URL(route.request().url()).pathname;
    return route.fulfill({ json: pathname === "/repos/owner/editor" ? { default_branch: "main" }
      : pathname.includes("/commits/") ? { sha: head, parents: [] }
      : pathname.includes("/git/trees/") ? { tree: [{ path, type: "blob", mode: "100644", sha: head, size: bytes.length }] }
      : pathname.includes("/git/blobs/") ? { size: bytes.length, encoding: "base64", content: bytes.toString("base64") } : [] });
  });
}

async function preview(page) {
  await page.goto("/?repo=owner%2Feditor");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  await page.locator("canvas").focus();
  await page.keyboard.press("Meta+p", { delay: 120 });
  const pad = page.locator("#review-dialog");
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: path, exact: true }));
  await expectClosed(page);
  await page.waitForTimeout(1500);
  await page.keyboard.press("Home", { delay: 120 });
  await page.waitForTimeout(500);
  await page.locator("canvas").evaluate(canvas => canvas.requestPointerLock());
  await page.keyboard.press("KeyP", { delay: 160 });
  await expectOpen(page, path);
  return pad;
}

// Given a read-only GitHub snapshot, when source is edited in the GPU tablet,
// private autosave restores edits after closing and reloading without a GitHub write.
test("VibePad edits and restores a private source draft with undo and Unicode paste", async ({ page }, info) => {
  await fixture(page);
  let pad = await preview(page);
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Edit", exact: true }));
  const editor = pad.locator("[data-code-editor]");
  await expect(editor).toBeAttached();
  await gpuClick(page, editor);
  await expect.poll(() => editor.getAttribute("data-editor-value")).not.toContain("unseen_tail");
  await page.keyboard.press("ControlOrMeta+End");
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.evaluate(() => navigator.clipboard.writeText("# λ → Привет\n"));
  await page.keyboard.press("ControlOrMeta+v", { delay: 120 });
  await expect.poll(() => editor.getAttribute("data-editor-value")).toContain("# λ → Привет");
  await page.keyboard.press("ControlOrMeta+z");
  await expect.poll(() => editor.getAttribute("data-editor-value")).not.toContain("Привет");
  await page.keyboard.press("ControlOrMeta+Shift+z");
  await expect.poll(() => editor.getAttribute("data-editor-value")).toContain("Привет");
  await page.keyboard.press("ControlOrMeta+s");
  await page.screenshot({ path: info.outputPath("source-editor.png") });
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Preview", exact: true }));
  await expect(pad.locator("pre")).toContainText("# λ → Привет");
  await expect(pad.locator("pre")).toContainText("unseen_tail");
  await page.keyboard.press("Tab", { delay: 160 });
  await expectClosed(page);
  pad = await preview(page);
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Edit", exact: true }));
  await expect.poll(async () => {
    const view = await readTablet(page);
    return view?.layout?.controls.find(control => control.kind === "editor")?.value;
  }).toContain("# λ → Привет");
});

test("wall search opens the matching chunk directly for editing", async ({ page }) => {
  await fixture(page);
  const pad = await preview(page);
  await page.keyboard.press("Tab", { delay: 160 });
  await expectClosed(page);
  await page.locator("canvas").focus();
  await page.locator("canvas").evaluate(canvas => canvas.requestPointerLock());
  await page.keyboard.press("Slash", { delay: 120 });
  await page.keyboard.type("unseen_tail", { delay: 40 });
  await page.keyboard.press("Enter", { delay: 160 });
  await expectOpen(page, path);
  const editor = pad.locator("[data-code-editor]");
  await expect(editor).toBeAttached();
  await expect.poll(() => editor.getAttribute("data-editor-value")).toContain("unseen_tail");
  await expect.poll(() => editor.getAttribute("data-editor-value")).not.toContain("def example");
});

test("Find opens another chunk immediately and keeps the earlier draft", async ({ page }) => {
  await fixture(page);
  const pad = await preview(page);
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Edit", exact: true }));
  const editor = pad.locator("[data-code-editor]");
  await gpuClick(page, editor);
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("# first chunk edit", { delay: 25 });
  await page.keyboard.press("ControlOrMeta+f");
  await page.keyboard.type("unseen_tail", { delay: 40 });
  await page.keyboard.press("Enter", { delay: 120 });
  await expect.poll(() => editor.getAttribute("data-editor-value")).toContain("unseen_tail");
  await expect.poll(() => editor.getAttribute("data-editor-value")).not.toContain("def example");
  await page.keyboard.press("ControlOrMeta+f");
  // Find is a native GPU field; wait for focus to leave the editor before
  // sending selection keys to that newly focused control.
  await expect.poll(async () => (await readTablet(page))?.layout?.focused).toBeNull();
  await page.keyboard.press("ControlOrMeta+a", { delay: 120 });
  await page.keyboard.type("def example", { delay: 40 });
  await page.keyboard.press("Enter", { delay: 120 });
  await expect.poll(() => editor.getAttribute("data-editor-value")).toContain("# first chunk edit");
  await expect.poll(() => editor.getAttribute("data-editor-value")).not.toContain("unseen_tail");
});

test("binary preview offers no text editing", async ({ page }) => {
  await fixture(page, true);
  const pad = await preview(page);
  await expect(pad).toContainText("Binary asset");
  await expect(pad.getByRole("button", { includeHidden: true, name: "Edit", exact: true })).toHaveCount(0);
  await expect(pad.locator("[data-code-editor]")).toHaveCount(0);
});

test("a full draft store keeps unsaved chunk edits in memory across tablet closure", async ({ page }) => {
  await fixture(page);
  await page.addInitScript(() => {
    const original = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (String(key).startsWith("repo-city-source-draft-v1:")) throw new DOMException("Test quota", "QuotaExceededError");
      return original.call(this, key, value);
    };
  });
  let pad = await preview(page);
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Edit", exact: true }));
  let editor = pad.locator("[data-code-editor]");
  await gpuClick(page, editor);
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("# unsaved private edit", { delay: 25 });
  await page.keyboard.press("Enter", { delay: 120 });
  await expect.poll(() => editor.getAttribute("data-editor-value")).toContain("unsaved private edit");
  await page.keyboard.press("ControlOrMeta+f");
  await page.keyboard.type("unseen_tail", { delay: 40 });
  await page.keyboard.press("Enter", { delay: 160 });
  await expect.poll(async () => (await readTablet(page))?.layout?.controls.find(control => control.kind === "editor")?.status).toContain("This chunk could not be saved");
  await expect.poll(() => editor.getAttribute("data-editor-value")).toContain("unsaved private edit");
  await expect.poll(() => editor.getAttribute("data-editor-value")).not.toContain("unseen_tail");
  await page.keyboard.press("Tab", { delay: 160 });
  await expectClosed(page);
  // Reopen inside this page: a reload cannot retain memory when storage is denied.
  await page.locator("canvas").focus();
  await page.locator("canvas").evaluate(canvas => canvas.requestPointerLock());
  await page.keyboard.press("KeyP", { delay: 160 });
  await expectOpen(page, path);
  await gpuClick(page, pad.getByRole("button", { includeHidden: true, name: "Edit", exact: true }));
  editor = pad.locator("[data-code-editor]");
  await expect.poll(() => editor.getAttribute("data-editor-value")).toContain("unsaved private edit");
});
