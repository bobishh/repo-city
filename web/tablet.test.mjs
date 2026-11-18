import assert from "node:assert/strict";
import { test } from "node:test";
import { screenCorners, screenTransform } from "./tablet.mjs";

test("Retina canvas pixels map to CSS pixels without enlarging the tablet UI", () => {
  const css = { left: 20, top: 10, width: 1280, height: 800 };
  const normal = [100, 60, 1180, 80, 1200, 740, 80, 720];
  const retina = normal.map(value => value * 2);
  assert.deepEqual(screenCorners(retina, 2560, 1600, css), screenCorners(normal, 1280, 800, css));
  assert.deepEqual(screenCorners(retina, 2560, 1600, css).slice(0, 2), [120, 70]);
});

test("tablet content follows all four corners of its physical screen", () => {
  const corners = [80, 60, 1150, 90, 1190, 720, 45, 700];
  const matrix = screenTransform(corners, 1440, 909);
  assert.equal(matrix.length, 16);
  for (const [index, x, y] of [[0, 0, 0], [1, 1440, 0], [2, 1440, 909], [3, 0, 909]]) {
    const w = matrix[3] * x + matrix[7] * y + matrix[15];
    const px = (matrix[0] * x + matrix[4] * y + matrix[12]) / w;
    const py = (matrix[1] * x + matrix[5] * y + matrix[13]) / w;
    assert.ok(Math.abs(px - corners[index * 2]) < 0.001);
    assert.ok(Math.abs(py - corners[index * 2 + 1]) < 0.001);
  }
});

test("a hidden or invalid screen cannot leave a stale interactive overlay", () => {
  assert.equal(screenTransform([], 1440, 909), null);
  assert.equal(screenTransform([0, 0, NaN, 0, 1, 1, 0, 1], 1440, 909), null);
  assert.equal(screenTransform([0, 0, 0, 0, 0, 0, 0, 0], 1440, 909), null);
});
