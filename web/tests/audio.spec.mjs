import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";

let server, origin;
test.beforeAll(async () => {
  const { createServer } = await import("node:http");
  server = createServer(async (request, response) => {
    const url = new URL(request.url, "http://localhost");
    if (["/audio.mjs", "/audio-worklet.mjs"].includes(url.pathname)) {
      response.writeHead(200, { "Content-Type": "text/javascript" });
      response.end(await readFile(new URL(`..${url.pathname}`, import.meta.url), "utf8"));
      return;
    }
    response.writeHead(200, { "Content-Type": "text/html" });
    response.end(`
      <button id="start">Start city</button><button id="picker">Open repository picker</button>
      <script type="module">
        import { createCityAudio } from '/audio.mjs';
        ${url.searchParams.has("unsupported") ? "window.AudioContext = undefined; window.webkitAudioContext = undefined;" : ""}
        window.pickerOpen = false;
        window.audio = createCityAudio({ active: () => !window.pickerOpen, onOutput(context, node) {
          window.analyser = context.createAnalyser(); analyser.fftSize = 1024; node.connect(analyser);
        }});
        document.querySelector('#picker').onclick = () => { window.pickerOpen = !window.pickerOpen; audio.refresh(); };
        window.signal = () => {
          if (!window.analyser) return 0;
          const samples = new Float32Array(analyser.fftSize); analyser.getFloatTimeDomainData(samples);
          return Math.sqrt(samples.reduce((total, sample) => total + sample * sample, 0) / samples.length);
        };
      </script>`);
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${server.address().port}`;
});
test.afterAll(async () => { await new Promise(resolve => server.close(resolve)); });

async function fixture(page, context, unsupported = false) {
  await page.goto(origin + (unsupported ? "/?unsupported=1" : "/"));
  await page.waitForFunction(() => Boolean(window.audio));
}

test("trusted city gesture starts real worklet audio; mute, off and repository picker silence it", async ({ page, context }) => {
  await fixture(page, context);
  expect(await page.evaluate(() => audio.available().state)).toBe("locked");
  await page.evaluate(() => { window.pickerOpen = true; });
  await page.getByRole("button", { name: "Start city" }).click();
  await expect.poll(() => page.evaluate(() => audio.available().state)).toBe("suspended");
  // Repository loading may finish after the trusted click; it must not require another click.
  await page.evaluate(() => { window.pickerOpen = false; audio.refresh(); });
  await expect.poll(() => page.evaluate(() => audio.available())).toMatchObject({ state: "running", error: "" });
  await expect.poll(() => page.evaluate(() => { const state = audio.available(); if (state.error) throw new Error(state.error); return signal(); })).toBeGreaterThan(0.0001);
  await page.evaluate(() => audio.updateSettings({ volume: 0 }));
  await expect.poll(() => page.evaluate(() => signal())).toBeLessThan(0.00001);
  await page.reload();
  await page.waitForFunction(() => Boolean(window.audio));
  expect(await page.evaluate(() => audio.settings().volume)).toBe(0);
  await page.evaluate(() => audio.updateSettings({ volume: 0.18 }));
  await page.getByRole("button", { name: "Start city" }).click();
  await expect.poll(() => page.evaluate(() => signal())).toBeGreaterThan(0.0001);
  await page.getByRole("button", { name: "Open repository picker" }).click();
  await expect.poll(() => page.evaluate(() => signal())).toBeLessThan(0.00001);
  await page.getByRole("button", { name: "Open repository picker" }).click();
  await expect.poll(() => page.evaluate(() => signal())).toBeGreaterThan(0.0001);
  await page.evaluate(() => audio.updateSettings({ ambience: false }));
  await expect.poll(() => page.evaluate(() => signal())).toBeLessThan(0.00001);
});

test("unsupported WebAudio leaves the city interactive", async ({ page, context }) => {
  await fixture(page, context, true);
  await page.getByRole("button", { name: "Start city" }).click();
  expect(await page.evaluate(() => audio.available().state)).toBe("unsupported");
  await page.getByRole("button", { name: "Open repository picker" }).click();
  expect(await page.evaluate(() => window.pickerOpen)).toBe(true);
});

test("visibility changes silence the real audio graph and resume it after returning", async ({ page, context }) => {
  await fixture(page, context);
  await page.getByRole("button", { name: "Start city" }).click();
  await expect.poll(() => page.evaluate(() => signal())).toBeGreaterThan(0.0001);
  // Headless Chromium keeps every tab visible; drive the same browser visibility event explicitly.
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect.poll(() => page.evaluate(() => signal())).toBeLessThan(0.00001);
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect.poll(() => page.evaluate(() => signal())).toBeGreaterThan(0.0001);
});
