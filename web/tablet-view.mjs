// Browser services own form state and commands. Bevy owns tablet pixels,
// layout and hit testing; no CSS transform renders this service projection.
export function tabletView(root, identify, images = new Map()) {
  function read(element) {
    if (element.classList.contains("markdown-end")) return { id: identify(element), kind: "end" };
    if (element.hidden || element.getAttribute("aria-hidden") === "true") return null;
    const tag = element.tagName.toLowerCase();
    const id = identify(element);
    const text = element.textContent?.trim() ?? "";
    const label = element.getAttribute("aria-label") ?? "";
    if (element.dataset.codeEditor !== undefined) return { id, kind: "editor", name: element.dataset.path, source: element.dataset.sourceKey, text: element.textContent ?? "", value: element.dataset.editorValue ?? element.textContent ?? "" };
    if (tag === "button" || tag === "a") return { id, kind: "button", text, name: label || text, disabled: element.disabled ?? false, selected: element.getAttribute("aria-pressed") === "true", color: element.querySelector(".paint-swatch")?.style.getPropertyValue("--pigment") ?? "" };
    if (tag === "input" || tag === "textarea" || tag === "select") {
      const kind = tag === "select" ? "select" : element.type === "checkbox" ? "check" : element.type === "range" ? "range" : element.type === "color" ? "color" : "input";
      return { id, kind, text: label || [...(element.closest("label")?.childNodes ?? [])].filter(node => node.nodeType === 3).map(node => node.textContent).join("").trim(), value: element.value, checked: Boolean(element.checked), disabled: Boolean(element.disabled), multiline: tag === "textarea", ...(element.maxLength > 0 ? { max_length: element.maxLength } : {}), ...(element.rows > 4 ? { rows: element.rows } : {}), min: element.min, max: element.max, step: element.step, options: tag === "select" ? [...element.options].map(option => ({ text: option.text, value: option.value })) : undefined };
    }
    if (element.dataset.markdownSource !== undefined) return { id, kind: "column", children: [{
      id: `${id}:markdown`, kind: "markdown", text: element.dataset.markdownSource,
      name: element.dataset.markdownPath, before: element.dataset.markdownBefore === "true",
    }, ...[...element.querySelectorAll(".markdown-end")].map(read)] };
    if (tag === "img") return { id, kind: "image", text: element.alt, source: element.dataset.previewSource ?? element.src, data: images.get(element.dataset.previewSource ?? element.src) ?? "" };
    if (/^h[1-6]$/.test(tag)) return { id, kind: "heading", text };
    if (element.classList.contains("settings-columns")) return { id, kind: "columns", children: children(element) };
    if (element.classList.contains("reading-limits")) return { id, kind: "row", children: children(element) };
    if (tag === "header") {
      const parts = children(element);
      if (element.classList.contains("vibepad-bar")) return { id, kind: "header", children: parts };
      return { id, kind: "row", children: [parts[0], { id: `${id}:spacer`, kind: "spacer" }, ...parts.slice(1)] };
    }
    if (element.matches("nav.vibepad-tabs")) return { id, kind: "tabs", children: children(element) };
    if (tag === "dl") return { id, kind: "controls", children: children(element) };
    if (element.matches("nav,.dialog-actions,.paint-slots > div,tr")) return { id, kind: "row", children: children(element) };
    if (element.matches("fieldset,.reading-estimate,form.review-reading")) return { id, kind: "card", children: children(element) };
    if (element.getAttribute("role") === "status" || element.matches(".review-status,.audio-status")) return { id, kind: "status", text };
    if (tag === "pre") return { id, kind: "code", text };
    if (tag === "label") {
      const control = element.querySelector("input,textarea,select");
      return { id, kind: "column", children: [...(control?.type === "checkbox" ? [] : [{ id: `${id}:label`, kind: "text", text: [...element.childNodes].filter(node => node.nodeType === 3).map(node => node.textContent).join("").trim() }]), ...[...element.children].map(read).filter(Boolean)] };
    }
    if (element.children.length === 0 || element.matches("p,dt,dd,li,th,td,legend,time,small")) return { id, kind: "text", text: tag === "li" ? `• ${text}` : text };
    return { id, kind: "column", children: children(element) };
  }
  function children(element) {
    return [...element.childNodes].flatMap((node, index) => node.nodeType === 3
      ? node.textContent.trim() ? [{ id: `${identify(element)}:text:${index}`, kind: "text", text: node.textContent.trim() }] : []
      : node.nodeType === 1 ? [read(node)].filter(Boolean) : []);
  }
  return children(root);
}
