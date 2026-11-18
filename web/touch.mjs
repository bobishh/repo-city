// Adapted from Feed the Poor touch.ts at beabe03f5aef648717fa175693cb9a76c953d9d5.
// Flight adds altitude, a held boost button, and tablet actions; no pointer lock is required.
export function createTouchControls({ canvas, document = globalThis.document, browser = globalThis, onPad } = {}) {
  const panel = document.createElement("nav");
  panel.id = "touch-controls";
  panel.setAttribute("aria-label", "Touch flight controls");
  const button = (label, name, parent = panel) => {
    const element = document.createElement("button");
    element.type = "button"; element.textContent = label; element.setAttribute("aria-label", name);
    parent.append(element); return element;
  };
  const stick = button("Move", "Move joystick"); stick.id = "move-stick";
  const thumb = document.createElement("span"); thumb.setAttribute("aria-hidden", "true"); stick.append(thumb);
  const tools = document.createElement("div"); tools.className = "touch-actions"; panel.append(tools);
  let move = [0, 0], look = [0, 0], movement, looking, origin, last, lookOrigin, swipe = 0, turned = false;
  const held = new Set(), pulses = [];
  const pulse = (label, name, key) => button(label, name, tools).addEventListener("pointerdown", event => {
    event.preventDefault(); event.stopPropagation(); if (key === "Tab" && onPad) onPad(); else pulses.push(key);
  });
  for (const [label, name, key] of [["Pad", "Open VibePad", "Tab"], ["Map", "Change map view", "KeyM"],
    ["Go", "Fly to aimed wall", "KeyE"], ["Read", "Preview aimed file", "KeyP"],
    ["Paint", "Choose paint", "KeyU"], ["Notes", "Review notes", "KeyJ"]]) pulse(label, name, key);
  const flight = document.createElement("div"); flight.className = "touch-altitude"; panel.append(flight);
  for (const [label, name, key] of [["Up", "Fly up", "Space"], ["Down", "Fly down", "ControlLeft"],
    ["Fast", "Fly faster", "ShiftLeft"], ["Use", "Use water or paint", "Fire"]]) {
    const element = button(label, name, flight); element.dataset.action = key;
    element.addEventListener("pointerdown", event => {
      event.preventDefault(); event.stopPropagation(); element.setPointerCapture(event.pointerId); held.add(key);
    });
    const release = () => held.delete(key);
    for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) element.addEventListener(type, release);
  }
  function reset() {
    move = [0, 0]; look = [0, 0]; movement = looking = undefined;
    held.clear(); pulses.length = 0; stick.style.setProperty("--x", "0px"); stick.style.setProperty("--y", "0px");
  }
  stick.addEventListener("pointerdown", event => {
    if (movement !== undefined) return;
    event.preventDefault(); movement = event.pointerId; origin = [event.clientX, event.clientY]; stick.setPointerCapture(movement);
  });
  stick.addEventListener("pointermove", event => {
    if (event.pointerId !== movement) return;
    const x = event.clientX - origin[0], y = event.clientY - origin[1], scale = Math.max(42, Math.hypot(x, y));
    move = [x / scale, -y / scale];
    stick.style.setProperty("--x", `${move[0] * 32}px`); stick.style.setProperty("--y", `${-move[1] * 32}px`);
  });
  for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) stick.addEventListener(type, event => {
    if (event.pointerId === movement) { movement = undefined; move = [0, 0]; stick.style.setProperty("--x", "0px"); stick.style.setProperty("--y", "0px"); }
  });
  canvas.addEventListener("pointerdown", event => {
    if (event.pointerType === "mouse" || looking !== undefined || panel.hidden) return;
    event.preventDefault(); looking = event.pointerId; lookOrigin = [event.clientX, event.clientY]; swipe = 0; turned = false; last = [event.clientX, event.clientY]; canvas.setPointerCapture(looking);
  });
  canvas.addEventListener("pointermove", event => {
    if (event.pointerId !== looking) return;
    const x = event.clientX - last[0], y = event.clientY - last[1];
    if (canvas.hasAttribute("data-touch-reading") && Math.hypot(...move) < 0.1) {
      swipe += x;
      if (!turned && Math.abs(swipe) > 48 && Math.abs(swipe) > Math.abs(event.clientY - lookOrigin[1]) * 1.5) {
        pulses.push(swipe < 0 ? "ArrowRight" : "ArrowLeft"); turned = true; look[1] = 0;
      }
      if (!turned) look[1] += y;
    } else { look[0] += x; look[1] += y; }
    last = [event.clientX, event.clientY];
  });
  for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) canvas.addEventListener(type, event => {
    if (event.pointerId === looking) looking = undefined;
  });
  function refresh() {
    const hidden = !browser.matchMedia("(pointer: coarse)").matches || canvas.hidden ||
      Boolean(document.querySelector("dialog[open]")) || document.hidden;
    if (hidden) reset();
    panel.hidden = hidden;
    canvas.toggleAttribute("data-touch-controls", !hidden);
  }
  document.body.append(panel);
  new browser.MutationObserver(refresh).observe(document.querySelector("main"), { attributes: true, subtree: true, attributeFilter: ["open", "hidden", "data-touch-reading"] });
  browser.addEventListener("blur", reset); document.addEventListener("visibilitychange", refresh);
  browser.matchMedia("(pointer: coarse)").addEventListener("change", refresh);
  refresh();
  return {
    take() {
      const value = { enabled: !panel.hidden, move, look, held: [...held], pulses: pulses.splice(0) };
      look = [0, 0]; return JSON.stringify(value);
    },
    reset,
  };
}
