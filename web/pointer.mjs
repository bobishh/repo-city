export function setupPointer(canvas, menu) {
  const locked = () => document.pointerLockElement === canvas;
  const modalOpen = () => menu.open || Boolean(document.querySelector("dialog[open]"));
  let pending = false;

  function release() {
    if (locked()) document.exitPointerLock();
  }

  function capture() {
    if (pending || canvas.hidden || modalOpen() || canvas.hasAttribute("data-pointer-ui") ||
        document.pointerLockElement || !canvas.requestPointerLock) return;
    canvas.focus({ preventScroll: true });
    pending = true;
    try {
      canvas.requestPointerLock()?.catch(() => { pending = false; });
    } catch {
      pending = false;
    }
  }

  document.addEventListener("pointerlockchange", () => {
    pending = false;
    canvas.toggleAttribute("data-pointer-locked", locked());
    if (modalOpen() || canvas.hasAttribute("data-pointer-ui")) release();
  });
  document.addEventListener("pointerlockerror", () => { pending = false; });
  window.addEventListener("blur", release);
  document.addEventListener("keydown", event => {
    if (event.code === "Escape") release();
  }, true);
  canvas.addEventListener("keydown", event => {
    if (event.code !== "Tab" || event.shiftKey || canvas.hasAttribute("data-pointer-ui")) return;
    event.preventDefault();
    if (!event.repeat) {
      if (locked()) release();
      else capture();
    }
  });
  canvas.addEventListener("pointerdown", event => {
    if (event.button === 0) capture();
  });
  canvas.addEventListener("contextmenu", event => {
    event.preventDefault();
  });
  return release;
}
