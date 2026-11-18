import assert from "node:assert/strict";
import { test } from "node:test";
import { setupPointer } from "./pointer.mjs";

test("overview clicks keep the cursor free while flight capture and Escape still work", () => {
  const previous = { document: globalThis.document, window: globalThis.window };
  const document = new EventTarget();
  const window = new EventTarget();
  const canvas = new EventTarget();
  const attributes = new Set(["data-overview"]);
  let captures = 0;
  document.querySelector = () => null;
  document.pointerLockElement = null;
  document.exitPointerLock = () => {
    document.pointerLockElement = null;
    document.dispatchEvent(new Event("pointerlockchange"));
  };
  canvas.focus = () => {};
  canvas.hasAttribute = name => attributes.has(name);
  canvas.toggleAttribute = (name, enabled) => enabled ? attributes.add(name) : attributes.delete(name);
  canvas.requestPointerLock = () => {
    captures++;
    document.pointerLockElement = canvas;
    document.dispatchEvent(new Event("pointerlockchange"));
    return Promise.resolve();
  };
  const click = () => canvas.dispatchEvent(Object.assign(new Event("pointerdown"), { button: 0 }));
  globalThis.document = document;
  globalThis.window = window;
  try {
    setupPointer(canvas, { open: false });
    click();
    assert.equal(captures, 0);
    assert.equal(document.pointerLockElement, null);
    assert.equal(attributes.has("data-pointer-ui"), false);
    attributes.delete("data-overview");
    click();
    assert.equal(captures, 1);
    assert.equal(document.pointerLockElement, canvas);
    document.dispatchEvent(Object.assign(new Event("keydown"), { code: "Escape" }));
    assert.equal(document.pointerLockElement, null);
    attributes.add("data-pointer-ui");
    click();
    assert.equal(captures, 1);
    attributes.delete("data-pointer-ui");
    click();
    assert.equal(captures, 2);
  } finally {
    globalThis.document = previous.document;
    globalThis.window = previous.window;
  }
});
