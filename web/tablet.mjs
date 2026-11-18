export function screenCorners(corners, width, height, bounds) {
  if (!(width > 0 && height > 0)) return [];
  return Array.from(corners, (value, index) => index % 2
    ? bounds.top + value * bounds.height / height
    : bounds.left + value * bounds.width / width);
}

// Map the browser UI onto the handheld screen drawn by the city renderer.
export function screenTransform(corners, width = 1440, height = 909) {
  if (corners.length !== 8 || !corners.every(Number.isFinite)) return null;
  const [x0, y0, x1, y1, x2, y2, x3, y3] = corners;
  const dx1 = x1 - x2, dx2 = x3 - x2, dx3 = x0 - x1 + x2 - x3;
  const dy1 = y1 - y2, dy2 = y3 - y2, dy3 = y0 - y1 + y2 - y3;
  const determinant = dx1 * dy2 - dx2 * dy1;
  if (Math.abs(determinant) < 0.0001) return null;
  const g = (dx3 * dy2 - dx2 * dy3) / determinant;
  const h = (dx1 * dy3 - dx3 * dy1) / determinant;
  return [
    (x1 - x0 + g * x1) / width, (y1 - y0 + g * y1) / width, 0, g / width,
    (x3 - x0 + h * x3) / height, (y3 - y0 + h * y3) / height, 0, h / height,
    0, 0, 1, 0, x0, y0, 0, 1,
  ];
}

export function createTablet(dialog, canvas) {
  let closingScreen;
  const removeSnapshot = () => { closingScreen?.remove(); closingScreen = undefined; };
  const visible = () => dialog.open && "tablet" in dialog.dataset;
  return {
    visible,
    raised: visible,
    open() {
      removeSnapshot();
      if (!dialog.open) {
        delete dialog.dataset.projected;
        dialog.showModal();
      }
    },
    close() {
      if (!visible()) return false;
      removeSnapshot();
      if ("projected" in dialog.dataset) {
        // Keep the last screen during the physical put-away animation, without a modal or input.
        closingScreen = document.createElement("div");
        closingScreen.className = dialog.className;
        closingScreen.style.cssText = dialog.style.cssText;
        Object.assign(closingScreen.dataset, dialog.dataset);
        closingScreen.setAttribute("open", "");
        closingScreen.append(...Array.from(dialog.childNodes, child => child.cloneNode(true)));
        closingScreen.id = "vibepad-closing-screen";
        closingScreen.inert = true;
        closingScreen.setAttribute("aria-hidden", "true");
        closingScreen.removeAttribute("aria-labelledby");
        for (const child of closingScreen.querySelectorAll("[id]")) child.removeAttribute("id");
        const inputs = dialog.querySelectorAll("input, textarea, select");
        closingScreen.querySelectorAll("input, textarea, select").forEach((copy, index) => {
          copy.value = inputs[index].value;
          if ("checked" in copy) copy.checked = inputs[index].checked;
        });
        document.body.append(closingScreen);
      }
      dialog.close();
      return true;
    },
    project(corners, width, height) {
      const projected = screenCorners(corners, width, height, canvas.getBoundingClientRect());
      const touch = globalThis.matchMedia?.("(pointer: coarse)").matches;
      const logicalWidth = touch && projected.length === 8 ? Math.max(360, Math.min(1440, Math.hypot(projected[2] - projected[0], projected[3] - projected[1]))) : 1440;
      const logicalHeight = logicalWidth * 909 / 1440;
      const matrix = screenTransform(projected, logicalWidth, logicalHeight);
      if (!matrix) {
        delete dialog.dataset.projected;
        removeSnapshot();
        return;
      }
      const surface = visible() ? dialog : closingScreen;
      if (surface) {
        surface.style.setProperty("--tablet-width", `${logicalWidth}px`);
        surface.style.setProperty("--tablet-height", `${logicalHeight}px`);
        surface.style.transform = `matrix3d(${matrix.join(",")})`;
        surface.dataset.projected = "";
      }
    },
  };
}
