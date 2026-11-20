import { test, expect } from "@playwright/test";

test("unread source shows blurred syntax through glass before washing", async ({ page }, info) => {
  const sha = "b".repeat(40);
  const path = "src/source.rs";
  const source = Array.from({ length: 24 }, (_, i) =>
    `    let message_${i} = "green syntax across the unread wall";`).join("\n") + "\n";
  const faults = [];
  page.on("pageerror", error => faults.push(error.message));
  page.on("console", message => {
    if (/Quitting the application|Caught rendering error/.test(message.text())) faults.push(message.text());
  });
  await page.addInitScript(() => {
    localStorage.setItem("repo-city-guide-v1", "seen");
    let locked = null;
    Object.defineProperty(document, "pointerLockElement", { get: () => locked });
    HTMLCanvasElement.prototype.requestPointerLock = function () {
      locked = this;
      queueMicrotask(() => document.dispatchEvent(new Event("pointerlockchange")));
      return Promise.resolve();
    };
  });
  await page.route("**/github/session", route => route.fulfill({ json: { enabled: false } }));
  await page.route("https://api.github.com/**", route => {
    const pathname = new URL(route.request().url()).pathname;
    const file = { path, type: "blob", mode: "100644", sha, size: source.length };
    return route.fulfill({ json: pathname === "/repos/owner/blur" ? { default_branch: "main" }
      : pathname.includes("/commits/") ? { sha, parents: [] }
      : pathname.includes("/git/trees/") ? { tree: [file] }
      : { encoding: "base64", content: Buffer.from(source).toString("base64"), size: source.length } });
  });
  await page.goto("/?repo=owner%2Fblur");
  await expect(page.locator("#start-screen")).toBeHidden({ timeout: 90_000 });
  const canvas = page.locator("canvas");
  await canvas.click();
  await expect(canvas).toHaveAttribute("data-pointer-locked", "");
  await page.keyboard.press("KeyE", { delay: 180 });
  const pixels = async () => {
    const png = await canvas.screenshot({ scale: "css", path: info.outputPath("unread-wall.png") });
    return page.evaluate(async encoded => {
      const bytes = Uint8Array.from(atob(encoded), char => char.charCodeAt(0));
      const image = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const context = new OffscreenCanvas(image.width, image.height).getContext("2d");
      context.drawImage(image, 0, 0);
      const { data } = context.getImageData(320, 300, 310, 380);
      image.close();
      let colored = 0;
      for (let i = 0; i < data.length; i += 4) {
        const red = data[i], green = data[i + 1], blue = data[i + 2];
        if (green > 60 && green > blue + 5 && green > red + 7) colored++;
      }
      return colored;
    }, png.toString("base64"));
  };
  await expect.poll(pixels, { timeout: 45_000 }).toBeGreaterThan(700);
  await expect(page.locator("#progress-summary")).toContainText("0 lines revealed");
  expect(faults).toEqual([]);
});
