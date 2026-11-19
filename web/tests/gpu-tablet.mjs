import { expect } from "@playwright/test";

export const readTablet = page => page.evaluate(() => globalThis.repoCity?.readTablet());
export const servicePanel = page => page.locator("#review-dialog");
export async function expectOpen(page, title) {
  await expect.poll(async () => {
    const view = await readTablet(page);
    const ids = new Set();
    const collect = nodes => { for (const node of nodes ?? []) { ids.add(node.id); collect(node.children); } };
    collect(view?.nodes);
    return Boolean(view?.open && (!title || view.title === title) && view.layout?.open && view.layout.controls.length && view.layout.controls.every(item => ids.has(item.id)));
  }).toBe(true);
}
export async function expectClosed(page) {
  await expect.poll(async () => Boolean((await readTablet(page))?.open)).toBe(false);
}
export async function gpuClick(page, control, touch = false) {
  await expect(control).toBeAttached();
  await expect.poll(() => control.getAttribute("data-tablet-control")).not.toBeNull();
  const id = await control.getAttribute("data-tablet-control");
  await expect.poll(async () => (await readTablet(page))?.layout?.controls.some(item => item.id === id && item.order >= 0)).toBe(true);
  let previous, stable = 0;
  for (let attempt = 0; attempt < 60; attempt++) {
    const view = await readTablet(page);
    const item = view.layout.controls.find(item => item.id === id && item.order >= 0);
    if (item?.visible) {
      const bounds = JSON.stringify(item.bounds);
      stable = previous === bounds ? stable + 1 : 0;
      previous = bounds;
      if (stable < 3) {
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
        continue;
      }
      const canvas = await page.locator("canvas").boundingBox();
      const point = [canvas.x + (item.bounds[0] + item.bounds[2] / 2) * canvas.width / view.layout.viewport[0], canvas.y + (item.bounds[1] + item.bounds[3] / 2) * canvas.height / view.layout.viewport[1]];
      if (touch) {
        const input = await page.context().newCDPSession(page);
        await input.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: point[0], y: point[1] }] });
        await page.waitForTimeout(100);
        await input.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
        await input.detach();
      } else await page.mouse.click(...point, { delay: 100 });
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      return;
    }
    await page.mouse.move(640, 400);
    await page.mouse.wheel(0, item?.bounds[1] < (item?.clip?.[1] ?? 0) ? -12 : 12);
    await expect.poll(async () => JSON.stringify((await readTablet(page)).layout.controls.find(item => item.id === id)?.bounds)).not.toBe(JSON.stringify(item?.bounds));
  }
  throw new Error(`Tablet control ${id} did not enter the screen`);
}
export async function gpuFill(page, control, value) {
  await gpuClick(page, control);
  await page.keyboard.press("ControlOrMeta+A", { delay: 120 });
  if (value) await page.keyboard.type(value, { delay: 30 });
  else await page.keyboard.press("Backspace", { delay: 100 });
}
export async function gpuSelect(page, control, option) {
  const text = await control.evaluate((element, option) => [...element.options].find(item => typeof option === "string" ? item.value === option : item.label === option.label)?.text, option);
  await gpuClick(page, control);
  await gpuClick(page, page.locator("[data-tablet-options]").getByRole("button", { name: text, exact: true, includeHidden: true }));
  await expect(control).toHaveValue(typeof option === "string" ? option : await control.evaluate((element, text) => [...element.options].find(item => item.text === text).value, text));
}
export async function gpuFocus(page, control) {
  const id = await control.getAttribute("data-tablet-control");
  await expect.poll(async () => (await readTablet(page))?.layout.focused).toBe(id);
}
