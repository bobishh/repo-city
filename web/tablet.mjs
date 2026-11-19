import { tabletView } from "./tablet-view.mjs";
export function createTablet(dialog, canvas) {
  let requestedFocus, dirty = true, counter = 0, state = { open: false, nodes: [] }, layout = { controls: [] };
  const ids = new WeakMap(), elements = new Map(), images = new Map();
  const identify = element => {
    if (!ids.has(element)) { const id = String(++counter); ids.set(element, id); elements.set(id, element); element.dataset.tabletControl = id; }
    return ids.get(element);
  };
  const visible = () => dialog.open && "tablet" in dialog.dataset;
  new MutationObserver(() => { dirty = true; }).observe(dialog, { childList: true, subtree: true, characterData: true, attributes: true });
  return {
    visible, raised: visible,
    focus(element) { requestedFocus = identify(element); dirty = true; },
    back() {
      const options = dialog.querySelector("[data-tablet-options]");
      if (!options) return false;
      requestedFocus = options.dataset.returnTo; options.remove(); dirty = true; return true;
    },
    open() {
      dialog.dataset.renderer = "bevy";
      dialog.setAttribute("open", "");
      canvas.focus({ preventScroll: true });
      dirty = true;
    },
    close() {
      if (!visible()) return false;
      dialog.close(); requestedFocus = undefined; images.clear(); dirty = true; return true;
    },
    view() {
      if (!dirty) return "";
      dirty = false;
      if (!visible()) { state = { open: false, nodes: [] }; return JSON.stringify(state); }
      for (const image of dialog.querySelectorAll("img")) {
        const source = image.src;
        if (!source.startsWith("blob:") || images.has(source)) continue;
        images.set(source, "");
        fetch(source).then(response => response.arrayBuffer()).then(buffer => {
          if (buffer.byteLength > 16 * 1024 * 1024) return;
          const bytes = new Uint8Array(buffer); let binary = "";
          for (let offset = 0; offset < bytes.length; offset += 16384) binary += String.fromCharCode(...bytes.subarray(offset, offset + 16384));
          if (image.isConnected && image.src === source) { images.set(source, btoa(binary)); dirty = true; }
        }).catch(() => { /* The preview's existing error state remains authoritative. */ });
      }
      const root = dialog.querySelector("[data-tablet-options]") ?? dialog;
      const nodes = tabletView(root, identify, images);
      for (const [id, element] of elements) if (!element.isConnected) elements.delete(id);
      const sources = new Set([...dialog.querySelectorAll("img")].map(image => image.src));
      for (const source of images.keys()) if (!sources.has(source)) images.delete(source);
      state = { open: true, title: root.querySelector("h2")?.textContent ?? "VibePad", focus: requestedFocus, nodes };
      requestedFocus = undefined;
      return JSON.stringify(state);
    },
    layout(value) { layout = JSON.parse(value); },
    inspect() { return { ...state, layout }; },
    action(value) {
      const event = JSON.parse(value);
      const element = elements.get(event.id);
      if (!element?.isConnected || element.disabled) return;
      if (event.kind === "editor") {
        // Rust owns the editing state. Keep this service projection observable
        // without replacing the source or rebuilding the GPU widget per keystroke.
        element.dataset.editorValue = event.value;
        if (event.range) element.dataset.editorRange = JSON.stringify(event.range);
      } else if (event.kind === "enter") {
        element.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true }));
      } else if (event.kind === "input") {
        element.value = event.value;
        element.dispatchEvent(new Event("input", { bubbles: true }));
        element.dispatchEvent(new Event("change", { bubbles: true }));
      } else if (event.kind === "check") {
        element.checked = !element.checked;
        element.dispatchEvent(new Event("change", { bubbles: true }));
      } else if (event.kind === "select") {
        const panel = document.createElement("section");
        panel.dataset.tabletOptions = ""; panel.dataset.returnTo = event.id;
        const title = document.createElement("h2"); title.textContent = element.closest("label")?.childNodes[0]?.textContent ?? "Choose an option";
        const back = document.createElement("button"); back.type = "button"; back.textContent = "Back";
        back.addEventListener("click", () => { panel.remove(); requestedFocus = event.id; dirty = true; });
        const search = document.createElement("input"); search.type = "search"; search.setAttribute("aria-label", "Filter options");
        const choices = document.createElement("div");
        const render = () => {
          choices.replaceChildren(...[...element.options].filter(option => option.text.toLowerCase().includes(search.value.trim().toLowerCase())).map(option => {
            const button = document.createElement("button"); button.type = "button"; button.textContent = option.text;
            button.setAttribute("aria-pressed", String(option.value === element.value));
            button.addEventListener("click", () => { element.value = option.value; element.dispatchEvent(new Event("change", { bubbles: true })); panel.remove(); requestedFocus = event.id; dirty = true; });
            return button;
          }));
          if (!choices.children.length) { const empty = document.createElement("p"); empty.setAttribute("role", "status"); empty.textContent = "No matching options."; choices.append(empty); }
          dirty = true;
        };
        search.addEventListener("input", render);
        search.addEventListener("keydown", event => { if (event.key === "Enter") choices.querySelector("button")?.click(); });
        panel.append(title, back, search, choices); dialog.append(panel); render();
        requestedFocus = identify(search);
      } else if (event.kind === "range") {
        const min = Number(element.min) || 0, max = Number(element.max) || 100;
        const step = (max - min) / 10;
        element.value = String(Number(element.value) >= max ? min : Math.min(max, Number(element.value) + step));
        element.dispatchEvent(new Event("input", { bubbles: true }));
      } else if (event.kind === "end") element.dispatchEvent(new Event("tabletread"));
      else element.click();
      dirty = true;
    },
  };
}
