// Adapted from Feed the Poor touch.ts at beabe03f5aef648717fa175693cb9a76c953d9d5.
// Repo City adds altitude, independent look, and tablet actions; no pointer lock is required.
// Circular controls follow Feed the Poor style.css at 6b1b2eed8a660bfb9dea2a4259175056d0718e06.
export function createTouchControls({ canvas, document = globalThis.document, browser = globalThis, onPad, enabled = () => true, walking = () => false } = {}) {
  const panel = document.createElement("nav");
  panel.id = "touch-controls";
  panel.setAttribute("aria-label", "Touch flight controls");
  const icons = {
    Pad: '<rect x="5" y="3" width="14" height="18"/><path d="M10 18h4"/>',
    Map: '<path d="m3 6 6-3 6 3 6-3v15l-6 3-6-3-6 3Zm6-3v15m6-12v15"/>',
    Go: '<path d="m3 10 18-7-7 18-3-8z"/>',
    Read: '<path d="M12 5v16M3 3l9 2 9-2v16l-9 2-9-2z"/>',
    Paint: '<path d="M8 7h8v14H8Zm1-4h6v4m3-4h3M18 6l3 2"/>',
    Notes: '<path d="M4 4h16v12H9l-5 5zM8 8h8m-8 4h5"/>',
    Up: '<path d="m5 14 7-7 7 7M12 7v14"/>',
    Down: '<path d="m5 10 7 7 7-7M12 3v14"/>',
    Fast: '<path d="m13 2-9 12h7l-1 8L21 9h-8z"/>',
    Use: '<path d="M12 2S4 11 4 15a8 8 0 0 0 16 0c0-4-8-13-8-13ZM8 15c0 2 1 4 4 4"/>',
    Move: '<path d="M12 3v18M3 12h18M8 7l4-4 4 4M8 17l4 4 4-4M7 8l-4 4 4 4m10-8 4 4-4 4"/>',
    Look: '<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>',
  };
  const svg = label => `<svg viewBox="0 0 24 24" aria-hidden="true">${icons[label]}</svg>`;
  const button = (label, name, parent = panel) => {
    const element = document.createElement("button");
    element.type = "button"; element.setAttribute("aria-label", name); element.title = name;
    element.innerHTML = svg(label) + `<span class="control-label">${label}</span>`;
    parent.append(element); return element;
  };
  const stick = button("Move", "Move joystick"); stick.id = "move-stick";
  const gazeStick = button("Look", "Look joystick"); gazeStick.id = "look-stick";
  for (const element of [stick, gazeStick]) {
    element.className = "touch-stick";
    element.innerHTML = `<span class="stick-ring"></span><span class="stick-thumb">${svg(element === stick ? "Move" : "Look")}</span>`;
  }
  const tools = document.createElement("div"); tools.className = "touch-actions"; panel.append(tools);
  const utility = document.createElement("div"); utility.className = "touch-utility"; panel.append(utility);
  let move = [0, 0], look = [0, 0], gaze = [0, 0], looking, last, lookOrigin, swipe = 0, turned = false;
  let sampled = browser.performance.now();
  const held = new Set(), pulses = [];
  const pulse = (label, name, key) => button(label, name, ["Tab", "KeyM", "KeyJ"].includes(key) ? utility : tools).addEventListener("pointerdown", event => {
    event.preventDefault(); event.stopPropagation(); if (key === "Tab" && onPad) onPad(); else pulses.push(walking() && key === "KeyJ" ? "KeyN" : key);
  });
  for (const [label, name, key] of [["Pad", "Open VibePad", "Tab"], ["Map", "Change map view", "KeyM"],
    ["Go", "Fly to aimed wall", "KeyE"], ["Read", "Preview aimed file", "KeyP"],
    ["Paint", "Choose paint", "KeyU"], ["Notes", "Review notes", "KeyJ"]]) pulse(label, name, key);
  const flight = document.createElement("div"); flight.className = "touch-altitude"; panel.append(flight);
  for (const [label, name, key] of [["Up", "Fly up", "Space"], ["Down", "Fly down", "ControlLeft"],
    ["Fast", "Fly faster", "ShiftLeft"], ["Use", "Use water or paint", "Fire"]]) {
    const element = button(label, name, flight); element.dataset.action = key;
    element.addEventListener("pointerdown", event => {
      event.preventDefault(); event.stopPropagation();
      if (walking() && key === "Fire") { pulses.push("KeyK"); return; }
      if (walking() && key === "Space" && !canvas.hasAttribute("data-library-flight")) { pulses.push("Space"); return; }
      if (walking() && key === "ControlLeft") { pulses.push("KeyO"); return; }
      element.setPointerCapture(event.pointerId); held.add(key);
    });
    const release = () => held.delete(key);
    for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) element.addEventListener(type, release);
  }
  function bindStick(element, apply, invertY = false) {
    let pointer, origin;
    const clear = () => {
      pointer = undefined; apply([0, 0]);
      element.style.setProperty("--x", "0px"); element.style.setProperty("--y", "0px");
      element.removeAttribute("data-active");
    };
    element.addEventListener("pointerdown", event => {
      if (pointer !== undefined) return;
      event.preventDefault(); pointer = event.pointerId; origin = [event.clientX, event.clientY];
      element.setPointerCapture(pointer); element.setAttribute("data-active", "");
    });
    element.addEventListener("pointermove", event => {
      if (event.pointerId !== pointer) return;
      const x = event.clientX - origin[0], y = event.clientY - origin[1], scale = Math.max(42, Math.hypot(x, y));
      apply([x / scale, (invertY ? -y : y) / scale]);
      element.style.setProperty("--x", `${x / scale * 30}px`); element.style.setProperty("--y", `${y / scale * 30}px`);
    });
    for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) element.addEventListener(type, event => {
      if (event.pointerId === pointer) clear();
    });
    return clear;
  }
  const clearMove = bindStick(stick, value => { move = value; }, true);
  const clearLook = bindStick(gazeStick, value => { gaze = value; });
  function reset() {
    clearMove(); clearLook(); look = [0, 0]; looking = undefined;
    held.clear(); pulses.length = 0; sampled = browser.performance.now();
  }
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
    } else { look[0] += x * 2; look[1] += y * 2; }
    last = [event.clientX, event.clientY];
  });
  for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) canvas.addEventListener(type, event => {
    if (event.pointerId === looking) looking = undefined;
  });
  function refresh() {
    const hidden = !enabled() ||
      !document.documentElement.hasAttribute("data-playing") ||
      !browser.matchMedia("(pointer: coarse)").matches || canvas.hidden ||
      Boolean(document.querySelector("dialog[open]")) || canvas.hasAttribute("data-library-map") || document.hidden;
    if (!hidden) {
      const foot = walking();
      panel.setAttribute("aria-label", foot ? "Touch walking controls" : "Touch flight controls");
      const use = flight.querySelector('[data-action="Fire"]');
      use.setAttribute("aria-label", foot ? "Use heat" : "Use water or paint");
      utility.querySelector("button").setAttribute("aria-label", foot ? "Open Stone Pad" : "Open VibePad");
      tools.querySelector("button").setAttribute("aria-label", foot ? "Open aimed scroll" : "Fly to aimed wall");
      const airborne = canvas.hasAttribute("data-library-flight");
      flight.querySelector('[data-action="Space"]').setAttribute("aria-label", foot && !airborne ? "Take off" : "Fly up");
      flight.querySelector('[data-action="ControlLeft"]').setAttribute("aria-label", foot ? "Land" : "Fly down");
      for (const element of flight.children) {
        const hide = foot && !airborne && ["ControlLeft", "ShiftLeft"].includes(element.dataset.action);
        if (element.hidden !== hide) element.hidden = hide;
      }
    }
    if (hidden) reset();
    if (panel.hidden !== hidden) panel.hidden = hidden;
    canvas.toggleAttribute("data-touch-controls", !hidden);
  }
  const viewport = canvas.parentElement;
  viewport.append(panel);
  const observer = new browser.MutationObserver(refresh);
  observer.observe(document.documentElement, { attributeFilter: ["data-playing"] });
  observer.observe(viewport, { subtree: true, attributeFilter: ["open", "hidden", "data-touch-reading", "data-library-map", "data-library-flight"] });
  browser.addEventListener("blur", reset); document.addEventListener("visibilitychange", refresh);
  browser.matchMedia("(pointer: coarse)").addEventListener("change", refresh);
  refresh();
  return {
    take() {
      const now = browser.performance.now(), seconds = Math.min(0.05, Math.max(0, (now - sampled) / 1000));
      sampled = now;
      const value = { enabled: !panel.hidden, move, look, aim: gaze.map(axis => axis * 800 * seconds), held: [...held], pulses: pulses.splice(0) };
      look = [0, 0]; return JSON.stringify(value);
    },
    reset,
  };
}
