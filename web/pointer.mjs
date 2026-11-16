// After Escape or a menu, a new canvas gesture can capture the pointer again.
export function setupPointer(canvas, menu) {
  let recapture = false;
  const locked = () => document.pointerLockElement === canvas;
  function capture() {
    if (menu.open || document.pointerLockElement) return;
    canvas.focus({ preventScroll: true });
    try {
      canvas.requestPointerLock()?.catch(() => {});
    } catch {
      // The browser can refuse an immediate relock after its Escape gesture.
    }
  }
  function release() {
    if (!locked()) return;
    recapture = true;
    document.exitPointerLock();
  }
  document.addEventListener("pointerlockchange", () => {
    if (locked()) recapture = true;
  });
  canvas.addEventListener("keydown", event => {
    if (event.code === "Tab" && !event.shiftKey) {
      event.preventDefault();
      if (event.repeat) return;
      if (locked()) release();
      else {
        recapture = true;
        capture();
      }
    }
    if (event.code === "Escape") release();
  });
  canvas.addEventListener("contextmenu", event => event.preventDefault());
  canvas.addEventListener("pointerdown", event => {
    if ((recapture && event.button === 0) || event.button === 2) capture();
  });
  return release;
}
