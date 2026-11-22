export function setupPointer(canvas, menu, enabled = () => true) {
  const locked = () => document.pointerLockElement === canvas;
  const modalOpen = () => menu.open || Boolean(document.querySelector("dialog[open]"));

  function release() {
    if (locked()) document.exitPointerLock();
  }

  function capture() {
    if (!enabled() || canvas.hidden || modalOpen() || canvas.hasAttribute("data-pointer-ui") || canvas.hasAttribute("data-overview") || canvas.hasAttribute("data-library-map") ||
        canvas.hasAttribute("data-library-reading") ||
        canvas.hasAttribute("data-action-menu") ||
        document.pointerLockElement || !canvas.requestPointerLock) return;
    canvas.focus({ preventScroll: true });
    try {
      canvas.requestPointerLock()?.catch(() => {});
    } catch { /* The next explicit click can retry. */ }
  }

  document.addEventListener("pointerlockchange", () => {
    canvas.toggleAttribute("data-pointer-locked", locked());
    if (modalOpen() || canvas.hasAttribute("data-pointer-ui")) release();
  });
  window.addEventListener("blur", release);
  document.addEventListener("keydown", event => {
    if (event.code === "Escape") release();
  }, true);
  canvas.addEventListener("pointerdown", event => {
    if (event.button === 0 && event.pointerType !== "touch") capture();
  });
  canvas.addEventListener("contextmenu", event => {
    event.preventDefault();
  });
  return release;
}
