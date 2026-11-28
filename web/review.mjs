import { loadCommitHistory, loadCommitDetails } from "./github.mjs";
import { createTablet } from "./tablet.mjs";
import { githubAccount, githubRequest, githubSignIn } from "./github-picker.mjs";
import { readCommentText, replacementLimit } from "./pr-review.mjs";
import { readProgress, writeProgress, readReview, saveReview, deleteReview } from "./progress.mjs";

const make = (tag, text, attributes) => {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  for (const [name, value] of Object.entries(attributes ?? {})) node.setAttribute(name, value);
  return node;
};
const button = (text, action) => {
  const element = make("button", text, { type: "button" });
  element.addEventListener("click", action);
  return element;
};
const hex = color => "#" + color.map(byte => byte.toString(16).padStart(2, "0")).join("");
const swatch = paint => {
  const ink = make("span", "", { class: "paint-swatch", "aria-hidden": "true" });
  ink.style.setProperty("--pigment", hex(paint.color));
  return ink;
};
const neutral = { id: "comment", label: "Comment", spray: "", prompt: "", color: [151, 199, 201] };
const link = (label, href) => make("a", label, { href, target: "_blank", rel: "noopener noreferrer" });
const pathLabel = note => note.anchor.start < 1 ? note.anchor.path :
  `${note.anchor.path}:${note.anchor.start}${note.anchor.start === note.anchor.end ? "" : "–" + note.anchor.end}`;
const current = (note, review) => note.revision === review.revision && note.baseline === review.baseline;
const mentions = (body, login) =>
  [...body.matchAll(/(^|[^\w@/.-])@([a-z\d-]+)(?![\w/-])/gi)]
    .some(([, , name]) => name.toLowerCase() === login.toLowerCase());
const uuid = () => crypto.randomUUID();
const changedPaths = ({ before, after }) => [...new Set([...Object.keys(before), ...Object.keys(after)])].filter(
  path => before[path]?.id !== after[path]?.id || before[path]?.mode !== after[path]?.mode,
);

export function createReview({ canvas, release, onClose, onTour, audio, onCompare = () => {}, onCanvas = () => {}, library = () => false, diving = () => false }) {
  const dialog = document.querySelector("#review-dialog");
  const tablet = createTablet(dialog, canvas);
  const settingsKey = "repo-city-reading-v1";
  let reading = { mode: "delayed", block_lines: 6, block_kind: "function", auto_advance: true, min_seconds: 3, max_seconds: 6, fly_to_font_px: 14 };
  try {
    if (localStorage.getItem("repo-city-wash-mode") === "immediate") reading.mode = "immediate";
    const previous = JSON.parse(localStorage.getItem(settingsKey));
    const stored = previous && { ...reading, ...previous };
    if (stored && !readingError(stored)) reading = stored;
  } catch { /* Use the built-in preferences. */ }
  let readingChanged = true;
  let session, version = 0, selected = 0, pendingSlot = -1, busy = false, destination, fileDestination;
  let markdownRead;
  let previewUrl;
  let historySelection = { before: null, after: null };
  let historyPage = 1;
  let traceText = "", traceBuiltText = "", traceSelected, traceRequest, traceReport, tracePending = false;
  const repaint = () => { version++; };
  let defaults = [];
  let renderParty = () => {}, partyPanelView;

  dialog.addEventListener("close", () => {
    if (dialog.open) return;
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    previewUrl = undefined;
    if (["markdown", "text", "source-editor", "image", "video"].includes(dialog.dataset.view)) {
      dialog.replaceChildren();
    }
    delete dialog.dataset.tablet;
    onClose();
  });
  dialog.addEventListener("cancel", event => {
    if (busy) event.preventDefault();
    else {
      if (dialog.dataset.view === "help") onTour(false);
      if ("tablet" in dialog.dataset) { event.preventDefault(); close(); }
    }
  });
  function selectTab(index) {
    if (!tablet.visible()) return false;
    const tab = dialog.querySelectorAll(".vibepad-tabs button")[index];
    if (!tab) return false;
    tab.click(); return true;
  }
  for (const type of ["keydown", "keyup"]) dialog.addEventListener(type, event => event.stopPropagation());
  canvas.addEventListener("wheel", event => {
    if (event.metaKey && document.pointerLockElement === canvas) event.preventDefault();
  }, { passive: false });

  function view(title, tag = "h2") {
    const heading = make(tag, title, { id: "review-title" });
    dialog.replaceChildren(heading);
    if ("tablet" in dialog.dataset) {
      const bar = make("header", undefined, { class: "vibepad-bar" });
      const gear = button("⚙", settings);
      gear.className = "vibepad-settings";
      gear.setAttribute("aria-label", "Settings");
      gear.title = "Settings";
      gear.setAttribute("aria-pressed", String(dialog.dataset.view === "settings"));
      const putAway = button("×", dismissTablet);
      const pad = library() ? "Stone Pad" : "VibePad";
      putAway.className = "vibepad-put-away"; putAway.setAttribute("aria-label", `Put away ${pad}`);
      bar.append(heading, gear, putAway);
      const tabs = make("nav", undefined, { "aria-label": pad, class: "vibepad-tabs" });
      for (const [label, page, open] of [
        ["Files", "changed-files", changedFiles],
        ["Find", "file-search", fileSearch],
        ["Git", "git-history", gitHistory],
        ["Comments", "notes", () => { notesPanel(); show(); }],
        ["Help", "help", help],
        ["Trace", "trace", tracePanel],
        ["Chat", "party", partyPanel],
      ]) {
        const tab = button(label, open);
        tab.setAttribute("aria-pressed", String(page === dialog.dataset.view));
        tabs.append(tab);
      }
      dialog.prepend(bar, tabs);
    }
    return dialog;
  }
  function actions(...children) {
    children = children.filter(Boolean);
    if (!children.length) return;
    const row = make("div", undefined, { class: "dialog-actions" });
    row.append(...children);
    dialog.append(row);
  }
  function show() {
    release();
    if ("tablet" in dialog.dataset) tablet.open();
    else if (!dialog.open) dialog.showModal();
  }
  function partyPanel() {
    partyPanelView?.dispose();
    dialog.dataset.view = "party";
    if (!canvas.hidden) dialog.dataset.tablet = "";
    view("Chat");
    partyPanelView = renderParty(dialog);
    show();
  }
  function close() {
    if (!tablet.close()) dialog.close();
  }
  function dismissTablet() {
    if (!tablet.visible()) return false;
    if (!busy) {
      if (dialog.dataset.view === "help") onTour(false);
      tablet.close();
    }
    return true;
  }
  const closeButton = () => "tablet" in dialog.dataset ? null : button("Close", close);
  const signIn = () => make("a", "Sign in with GitHub", { href: githubSignIn() });

  function markdown(text, container = make("div", undefined, { class: "markdown-preview" })) {
    container.dataset.markdownSource = text;
    container.textContent = text;
    return container;
  }

  function sourceEditor(path, text, identity, preview) {
    dialog.dataset.view = "source-editor";
    dialog.dataset.tablet = "";
    view(path);
    const editor = make("div", text, { "data-code-editor": "", "data-path": path,
      "data-source-key": JSON.stringify(identity), "aria-label": `Edit ${path}` });
    actions(button("Preview", () => {
      const range = JSON.parse(editor.dataset.editorRange ?? JSON.stringify(identity.range));
      const lines = text.match(/[^\n]*\n|[^\n]+$/g) ?? [];
      const value = editor.dataset.editorValue ?? lines.slice(range.start, range.end).join("");
      preview({ range, value, displayText: lines.slice(0, range.start).join("") + value + lines.slice(range.end).join("") });
    }));
    dialog.append(editor);
    show();
    tablet.focus(editor);
  }

  function previewMarkdown(path, load, kind = "markdown", edit = false) {
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    previewUrl = undefined;
    dialog.dataset.view = kind;
    if (kind === "video") delete dialog.dataset.tablet;
    else dialog.dataset.tablet = "";
    view(path);
    let content = make("article", kind === "video" ? "Loading video…" : "Loading document…",
      { class: "markdown-preview", "aria-live": "polite" });
    dialog.append(content);
    if (kind === "video") actions(closeButton());
    show();
    void load().then(({ text, mark, bytes, format, identity, displayText, pageOffsets }) => {
      if (!dialog.open || !content.isConnected) return;
      if (kind === "video") {
        const types = { mp4: "video/mp4", m4v: "video/mp4", mov: "video/quicktime", webm: "video/webm", ogv: "video/ogg" };
        const video = make("video", undefined, { controls: "", playsinline: "", preload: "metadata", "aria-label": path });
        video.addEventListener("error", () => { content.textContent = "This video could not be played."; }, { once: true });
        previewUrl = URL.createObjectURL(new Blob([bytes], { type: types[format] }));
        video.src = previewUrl;
        content.replaceChildren(video);
        return;
      }
      if (kind === "image") {
        const image = make("img", undefined, { alt: path, class: "file-image-preview" });
        // The common Rust decoder validates dimensions and formats. Browser-native image
        // decoding is not authoritative: several supported raster formats have no HTML decoder.
        const types = {jpg:"image/jpeg",jpeg:"image/jpeg",tif:"image/tiff",tiff:"image/tiff",ico:"image/vnd.microsoft.icon",tga:"image/x-tga",pnm:"image/x-portable-anymap",pbm:"image/x-portable-bitmap",pgm:"image/x-portable-graymap",ppm:"image/x-portable-pixmap",pam:"image/x-portable-arbitrarymap",dds:"image/vnd-ms.dds",hdr:"image/vnd.radiance",rgbe:"image/vnd.radiance"};
        previewUrl = URL.createObjectURL(new Blob([bytes], { type: types[format] ?? `image/${format}` }));
        image.dataset.previewSource = previewUrl;
        content.replaceChildren(image);
        return;
      }
      if (identity) {
        const openEditor = () => sourceEditor(path, text, identity, ({ range, displayText }) =>
          previewMarkdown(path, async () => ({ text, mark, pageOffsets, identity: { ...identity, range }, displayText }), kind));
        if (edit) { openEditor(); return; }
        const editButton = button("Edit", openEditor);
        content.before(editButton);
      }
      const visibleText = displayText ?? text;
      if (!visibleText.trim()) { content.textContent = "Empty document."; return; }
      const offsets = pageOffsets?.(visibleText) ?? [0, visibleText.length];
      const pages = offsets.length - 1;
      const read = new Set();
      let page = 0;
      const previous = button("Previous page", () => showPage(page - 1));
      const next = button("Next page", () => showPage(page + 1));
      const counter = make("span", "", { role: "status" });
      if (pages > 1) {
        const navigation = make("div", undefined, { class: "dialog-actions" });
        navigation.append(previous, counter, next);
        content.before(navigation);
      }
      function showPage(index) {
        page = index;
        previous.disabled = page === 0;
        next.disabled = page === pages - 1;
        counter.textContent = `Page ${page + 1} of ${pages}`;
        const section = make("article", undefined, { class: "markdown-preview", "aria-live": "polite" });
        const chunk = visibleText.slice(offsets[index], offsets[index + 1]);
        if (kind === "text") {
          section.append(make("pre", chunk, { class: "plain-file-preview", "data-source-path": path }));
        } else {
          section.dataset.markdownPath = path;
          section.dataset.markdownBefore = String(mark?.before ?? false);
          markdown(chunk, section);
        }
        const end = make("div", undefined, { class: "markdown-end", "aria-hidden": "true" });
        section.append(end);
        end.addEventListener("tabletread", () => {
          read.add(index);
          if (displayText === undefined && read.size === pages) markdownRead = mark;
        });
        content.replaceWith(section);
        content = section;
      }
      showPage(0);
    }).catch(error => {
      if (content.isConnected) content.textContent = error.message;
    });
  }

  function help() {
    dialog.dataset.view = "help";
    dialog.dataset.tablet = "";
    view("Help");
    if (diving()) {
      dialog.append(make("p", "The lamp guides you between stations. Up close the reading surface lights evenly. Keep a block in view to read it. Read lines keep glowing."));
      const controls = make("dl", undefined, { class: "review-controls" });
      for (const [keys, purpose] of [
        ["WASD / mouse", "Steer the bathyscaphe and aim the lamp"],
        ["Space / Ctrl / Shift", "Ascend, descend, or boost the thrusters"],
        ["E", "Approach the file in your sights"],
        ["O", "Drop a survey buoy or retrieve a nearby one"],
        ["0", "Select the reading lamp"],
        ["1–9 / drag", "Select a paint and mark readable lines"],
        ["U / J", "Edit paints or open review notes"],
        ["/", "Find text at a file or find a file"],
        ["G / B", "Read the diff or blame"],
        ["Tab / P", "Open VibePad or preview a file"],
      ]) controls.append(make("dt", keys), make("dd", purpose));
      dialog.append(controls);
      show();
      return;
    }
    if (library()) {
      dialog.append(make("p", "Click Repo Polis to open the repository picker."));
      const controls = make("dl", undefined, { class: "review-controls" });
      for (const [keys, purpose] of [
        ["WASD / mouse", "Walk and look around"],
        ["Space / O", "Take off or land in front of a reading room"],
        ["Right-click / Shift+F10", "Open actions for the current target"],
        ["E / M", "Open the aimed scroll or cycle the cabinet and library maps"],
        ["Tab / Cmd+P / P", "Open Stone Pad, find a file, or preview the scroll"],
        ["G / B / Q", "Compare, open blame, or switch snapshot side"],
        ["Scroll / J", "Move along the scroll or jump to a line"],
        ["/ / Z / X", "Find text, fold a block, or unfold it"],
        ["C / [ / ]", "Start changed-file review or move between hunks"],
        ["K / 0–9 / U", "Use heat or paint, select a paint, or edit paints"],
        ["Shift+J / Esc", "Open comments on the aimed line or close the scroll"],
      ]) controls.append(make("dt", keys), make("dd", purpose));
      dialog.append(controls);
      actions(button("Start tour", () => { onTour(true); close(); }));
      show();
      return;
    }
    dialog.append(make("p", "Click the Repo City logo to open the repository picker."));
    const controls = make("dl", undefined, { class: "review-controls" });
    for (const [keys, purpose] of [
      ["WASD / Space / Ctrl", "Fly and change altitude"],
      ["O", "Land and walk; O or Space to fly again"],
      ["Arrows / Shift+Arrows", "Look around; up close, scroll or turn to another wall. Shift keeps free look."],
      ["K", "Use heat or hold to paint"],
      ["E / M", "Fly to the aimed wall or switch overview"],
      ["Tab / P", "Open VibePad or preview the aimed file"],
      ["/", "Find text on the aimed wall or find a file"],
      ["N / Shift+N", "Cycle changed files or wall search matches"],
      ["Cmd/Ctrl+P", "Find a file"],
      ["Z / X", "Collapse or expand the aimed code block"],
      ["H", "Open or put away Help"],
      ["Alt+1–5 / Arrows / Enter", "Select a VibePad tab, move through controls, or activate"],
      ["Ctrl+Tab / Shift+Tab", "Next or previous control, including text fields"],
      ["F / G / B", "Filter changed files, open diff, or open blame"],
      ["Left / Right", "Cycle source, diff and blame at a close wall"],
      ["Touch swipe", "Up/down reads along the wall; sideways turns to another face. The joystick flies away."],
      ["Scroll", "Read along a wall or move between review stops"],
      ["0 / 1–9 / U", "Select heat, select paint, or edit paints"],
      ["C / J", "Follow changed lines or open review comments"],
    ]) controls.append(make("dt", keys), make("dd", purpose));
    dialog.append(controls);
    const finish = started => { onTour(started); close(); };
    actions(button("Skip", () => finish(false)), button("Start tour", () => finish(true)));
    show();
  }

  function readingError(values) {
    if (!Number.isInteger(values.block_lines) || values.block_lines < 1 || values.block_lines > 4294967295) {
      return "Lines per reading block must be a positive whole number.";
    }
    if (!Number.isInteger(values.fly_to_font_px) || values.fly_to_font_px < 8 || values.fly_to_font_px > 32) {
      return "Enter a fly-to font size from 8 to 32 px.";
    }
    if (!["delayed", "immediate"].includes(values.mode) ||
        [values.min_seconds, values.max_seconds].some(value => value !== null &&
          (!Number.isFinite(value) || value < 0 || value > 3.4028234e38))) {
      return "Enter non-negative seconds or leave the field blank.";
    }
    if (values.min_seconds !== null && values.max_seconds !== null && values.min_seconds > values.max_seconds) {
      return "Minimum delay must not exceed maximum delay.";
    }
    return "";
  }

  function soundSettings() {
    const section = make("fieldset", undefined, { class: "review-audio" });
    section.append(make("legend", "Sound"));
    const preferences = audio?.settings() ?? { ambience: false, speech: false, volume: 0.18 };
    const available = audio?.available() ?? { supported: false, speech: false };
    const feedback = make("p", "", { class: "audio-status" });
    const controls = [];
    const apply = next => {
      try { audio?.updateSettings(next); feedback.textContent = ""; }
      catch (error) { feedback.textContent = error.message; }
    };
    for (const [key, label] of [["ambience", "City ambience"], ["speech", "Robot speech"]]) {
      const input = make("input", undefined, { type: "checkbox" });
      input.checked = preferences[key];
      input.disabled = !available.supported || (key === "speech" && !available.speech);
      input.addEventListener("change", () => apply({ [key]: input.checked }));
      const row = make("label", undefined, { class: "review-toggle" });
      row.append(input, label);
      controls.push(row);
    }
    const volume = make("input", undefined, { type: "range", min: "0", max: "100", step: "1" });
    volume.value = Math.round(preferences.volume * 100);
    volume.disabled = !available.supported;
    volume.addEventListener("input", () => apply({ volume: Number(volume.value) / 100 }));
    section.append(...controls, field("Volume", volume), feedback);
    const radio = make("select");
    for (const [value, label] of [["off", "Off"], ["space_trance", "Space trance"], ["techno", "Techno"]]) {
      radio.append(make("option", label, { value }));
    }
    radio.value = preferences.radio ?? "off";
    radio.disabled = !available.supported;
    radio.addEventListener("change", () => apply({ radio: radio.value }));
    section.append(field("Radio", radio));
    if (!available.supported) feedback.textContent = "This browser cannot synthesize city audio.";
    else if (available.error) feedback.textContent = available.error;
    else if (!available.speech) feedback.textContent = "Robot speech needs an installed local browser voice.";
    const voices = audio?.voices?.() ?? [];
    if (voices.length) {
      const voice = make("select");
      for (const entry of voices) voice.append(make("option", `${entry.name} (${entry.lang})`, { value: entry.name }));
      voice.value = voices.some(entry => entry.name === preferences.voice) ? preferences.voice : "";
      voice.addEventListener("change", () => apply({ voice: voice.value }));
      section.append(field("Voice", voice));
    }
    section.append(make("p", "Reads the current block from start to end. Tool sound is muted; ambience and radio become quieter during speech. Radio music is synthesized on this device."));
    return section;
  }

  function settings() {
    dialog.dataset.view = "settings";
    dialog.dataset.tablet = "";
    view("Settings");
    const columns = make("div", undefined, { class: "settings-columns" });
    const form = make("form", undefined, { class: "review-reading" });
    const mode = make("select");
    for (const value of ["delayed", "immediate"]) {
      mode.append(make("option", value === "delayed" ? "Delayed" : "Immediate", { value }));
    }
    mode.value = reading.mode;
    const kind = make("select");
    for (const [value, label] of [["function", "Whole function"], ["lines", "Fixed lines"]]) kind.append(make("option", label, { value }));
    kind.value = reading.block_kind ?? "function";
    const auto = make("input", undefined, { type: "checkbox" }); auto.checked = reading.auto_advance ?? true;
    const showSource = make("input", undefined, { type: "checkbox" }); showSource.checked = reading.show_all_source ?? false;
    const block = make("input", undefined, { type: "number", min: "1", step: "1", value: reading.block_lines });
    const bound = value => make("input", undefined, { type: "text", inputmode: "decimal", value: value ?? "", placeholder: "No limit" });
    const min = bound(reading.min_seconds), max = bound(reading.max_seconds);
    const fontSize = make("input", undefined, { type: "number", min: "8", max: "32", step: "1", value: reading.fly_to_font_px });
    const status = make("p", "", { class: "review-status", role: "status" });
    form.noValidate = true;
    const limits = make("div", undefined, { class: "reading-limits" });
    limits.append(field("Minimum delay (seconds)", min), field("Maximum delay (seconds)", max));
    form.append(make("h3", "Reading"), field("Source always visible", showSource), field("Read credit", mode), field("Reading block", kind), field("Lines per reading block", block),
      make("p", "Used for fixed blocks and when function boundaries are unavailable."),
      field("Auto-advance downward", auto), limits,
      make("p", "Block size and time limits are independent. Leave either limit blank to use the calculated time."),
      field("City fly-to font size (px)", fontSize), status);
    status.textContent = "Changes save automatically.";
    const estimate = make("section", undefined, { class: "reading-estimate" });
    estimate.append(make("h3", "Reading estimate"), make("code", "seconds = characters × 60 / (238 × 4.6)"),
      make("p", "Counts non-whitespace characters; consecutive copies count once. Six lines is a configurable preset, not a measured optimum."),
      make("p", "Brysbaert (2019) measured English nonfiction, not code comprehension. This length-adjusted estimate is an approximation."));
    const side = make("div", undefined, { class: "settings-side" }); side.append(soundSettings(), estimate);
    const applyReading = () => {
      const seconds = input => input.value.trim() === "" ? null : Number(input.value);
      const next = { mode: mode.value, block_kind: kind.value, auto_advance: auto.checked, show_all_source: showSource.checked, block_lines: Number(block.value), min_seconds: seconds(min), max_seconds: seconds(max), fly_to_font_px: Number(fontSize.value) };
      const error = readingError(next);
      if (error) { status.textContent = error; return; }
      reading = next;
      readingChanged = true;
      try {
        localStorage.setItem(settingsKey, JSON.stringify(reading));
        status.textContent = "Saved automatically.";
      } catch {
        status.textContent = "Applied for this session. Browser storage is unavailable.";
      }
    };
    form.addEventListener("input", applyReading);
    form.addEventListener("change", applyReading);
    form.addEventListener("submit", event => { event.preventDefault(); applyReading(); });
    columns.append(form, side); dialog.append(columns);
    show();
  }

  async function load(repository) {
    const { review, progressScope } = repository;
    const snapshot = JSON.parse(repository.snapshot);
    const account = githubAccount()?.login ?? null;
    const project = review.repository.toLowerCase();
    const scope = JSON.stringify(["notes", account, project, review.pull]);
    const projectScope = JSON.stringify(["project", account, project]);
    const data = { review, scope, projectScope, progressScope, snapshot, account, notes: [], threads: [], palette: [],
      loadChangeDates: repository.loadChangeDates, threadPaints: new Map(), showResolved: false, pending: Boolean(review.pull), status: "", prior: null };
    const [notes, prefs] = await Promise.all([readReview(scope), readReview(projectScope)]).catch(() => {
      data.status = "Local review storage is unavailable.";
      return [[], []];
    });
    data.notes = notes.filter(entry => entry?.anchor);
    data.palette = prefs.find(entry => entry.palette)?.palette ?? [];
    data.prior = notes.find(entry => entry.lastHead) ?? null;
    data.showResolved = notes.some(entry => entry.showResolved === true);
    for (const entry of notes) {
      if (entry.threadId && entry.paint) data.threadPaints.set(entry.threadId, entry.paint);
    }
    const prior = data.prior;
    // Carry only progress with a proven identical blob on the relevant side.
    if (prior?.progressScope && prior.lastHead !== review.revision) {
      try {
        const carried = [];
        for (const json of await readProgress(prior.progressScope)) {
          const { reveal, read_document, reviewed } = JSON.parse(json);
          const source = reveal ?? read_document;
          if (source) {
            const side = source.before ? "before" : "after";
            const old = read_document?.blob ?? prior.blobs?.[source.path]?.[side];
            if (old && old === snapshot[side][source.path]?.id) carried.push([json, true]);
          } else if (reviewed) {
            const old = prior.blobs?.[reviewed];
            if (old && old.before === (snapshot.before[reviewed]?.id ?? null) &&
                old.after === (snapshot.after[reviewed]?.id ?? null)) carried.push([json, true]);
          }
        }
        if (carried.length && !notes.some(entry => entry.carried === review.revision)) {
          await writeProgress(progressScope, carried);
          await saveReview(scope, "carried-" + review.revision, { carried: review.revision });
        }
      } catch { data.status = "Couldn't carry progress from the previous review."; }
    }
    session = data;
    traceText = ""; traceReport = undefined; traceRequest = undefined; tracePending = false;
    traceBuiltText = ""; traceSelected = undefined;
    historySelection = { before: data.review?.baseline ?? null, after: data.review?.revision ?? null }; historyPage = 1;
    destination = undefined;
    selected = 0;
    pendingSlot = 0;
    repaint();
    if (review.pull) void loadThreads(data);
  }

  async function loadThreadStatus(review) {
    const result = new Map();
    if (!githubAccount()?.login) return result;
    try {
      let cursor = null;
      for (let page = 1; page <= 12; page++) {
        const suffix = cursor ? "?" + new URLSearchParams({ cursor }) : "";
        const response = await fetch(`/github/review/${review.repository}/${review.pull}${suffix}`, {
          signal: AbortSignal.timeout(12_000),
        });
        if (!response.ok) break;
        const { headRefOid, reviewThreads } = await response.json();
        for (const thread of reviewThreads.nodes) {
          const id = thread?.comments.nodes[0]?.id;
          if (id) result.set(id, { ...thread, isOutdated: thread.isOutdated || headRefOid !== review.revision });
        }
        const next = reviewThreads.pageInfo;
        if (!next.hasNextPage || !next.endCursor || next.endCursor === cursor) break;
        cursor = next.endCursor;
      }
    } catch { /* REST threads remain usable without resolution status. */ }
    return result;
  }

  async function loadThreads(data) {
    const { review } = data, replies = new Map();
    const statuses = loadThreadStatus(review);
    try {
      const items = [];
      for (let page = 1; page <= 12; page++) {
        const response = await githubRequest(
          `https://api.github.com/repos/${review.repository}/pulls/${review.pull}/comments?per_page=100&page=${page}`,
          { signal: AbortSignal.timeout(12_000) },
        );
        if (!response.ok) throw new Error("GitHub review comments are unavailable.");
        const batch = await response.json();
        if (!Array.isArray(batch)) throw new Error("GitHub review comments are unavailable.");
        items.push(...batch);
        if (!response.headers.get("Link")?.includes('rel="next"')) break;
      }
      for (const item of items) if (item.in_reply_to_id) {
        const group = replies.get(item.in_reply_to_id) ?? [];
        group.push(item);
        replies.set(item.in_reply_to_id, group);
      }
      const states = await statuses;
      data.threads = items.filter(item => !item.in_reply_to_id && typeof item.path === "string").map(item => {
        const parsed = readCommentText(item.body);
        const state = states.get(item.node_id);
        const line = state?.line ?? item.line;
        const start = state?.startLine ?? item.start_line;
        const side = state?.diffSide ?? item.side;
        const startSide = state?.startDiffSide ?? item.start_side ?? side;
        const visible = !(state?.isOutdated ?? item.position === null) && Number.isSafeInteger(line) && line > 0;
        const anchored = visible && startSide === side && (side === "RIGHT" || side === "LEFT");
        const end = anchored ? line : state?.originalLine ?? item.original_line ?? 0;
        const discussion = [item, ...(replies.get(item.id) ?? [])];
        const addressed = anchored && state?.isResolved === false && data.account
          ? discussion.filter(comment => mentions(readCommentText(comment.body).text, data.account))
          : [];
        const reactions = new Map(state?.comments.nodes.map(comment => [comment.id, comment.reactionGroups ?? []]));
        const personal = addressed.filter(comment => reactions.has(comment.node_id)).map(comment => ({
          id: comment.id,
          viewed: reactions.get(comment.node_id).some(group => group.content === "EYES" && group.viewerHasReacted),
        }));
        return {
          id: `github-${item.id}`, revision: anchored ? review.revision : item.original_commit_id ?? review.revision,
          baseline: review.baseline, remote: true, preset: neutral, presetId: parsed.preset,
          anchor: { path: state?.path ?? item.path, before: side === "LEFT",
            start: anchored ? start ?? end : state?.originalStartLine ?? item.original_start_line ?? end, end },
          text: parsed.text, original: !anchored, fileLevel: item.subject_type === "file", resolved: state?.isResolved ?? null,
          discussion, personal,
        };
      });
    } catch (error) {
      if (!data.status) data.status = error.message;
    } finally {
      data.pending = false;
      if (session === data) {
        repaint();
        if (dialog.open && dialog.dataset.view === "notes") notesPanel();
      }
    }
  }

  function notes() {
    if (!session) return [];
    const palette = session.palette.length ? session.palette : defaults;
    const localByUrl = new Map(session.notes.filter(note => note.published).map(note => [note.published, note]));
    const found = new Set();
    const threads = session.threads.flatMap(note => {
      const url = safeDiscussion(note.discussion[0]?.html_url);
      const local = localByUrl.get(url);
      if (local) found.add(local.id);
      if (note.resolved && !session.showResolved) return [];
      const choice = session.threadPaints.get(note.id);
      const paint = choice ?? local?.preset ?? palette.find(preset => preset?.id === note.presetId) ?? neutral;
      const preset = note.resolved
        ? { ...paint, color: paint.color.map(channel => Math.round(channel * 0.55 + 36)) }
        : paint;
      return [{ ...note, preset, attention: note.personal.some(comment => !comment.viewed) }];
    });
    return [...session.notes.filter(note => !found.has(note.id)), ...threads];
  }
  function snapshot() {
    return JSON.stringify({ palette: session?.palette ?? [], notes: notes() });
  }
  function safeDiscussion(url) {
    try { const parsed = new URL(url); return parsed.origin === "https://github.com" ? parsed.href : null; }
    catch { return null; }
  }
  function allowedDraft() { return Boolean(session?.review.pull) && Boolean(githubAccount()?.login); }
  function add(json) {
    if (!allowedDraft()) {
      if (session?.review.pull) return "Sign in with GitHub to leave a PR note.";
      return "Open a pull request to leave a note in the browser.";
    }
    const data = session;
    const draft = JSON.parse(json);
    if (!current(draft, data.review)) return "The revision changed. Mark the lines again.";
    const index = data.notes.findIndex(existing => existing.id === draft.id);
    const note = { ...data.notes[index], ...draft, text: draft.text || draft.preset.prompt };
    if (index >= 0) data.notes[index] = note;
    else data.notes.push(note);
    repaint();
    void saveReview(data.scope, note.id, note).catch(() => {
      data.status = "Could not save the draft to this browser; it remains open in this session.";
      if (session === data && dialog.open && dialog.dataset.view === "notes") notesPanel();
    });
    return "";
  }

  function paintsPanel() {
    dialog.dataset.view = "paints";
    view("Paints");
    dialog.append(make("p", diving()
      ? "Read a block at a station, then drag a paint across its lines to mark them."
      : "Pick a tool. Thaw source first, then mark a range with a paint."));
    const grid = make("div", undefined, { class: "paint-slots" });
    const palette = session?.palette.length ? session.palette : defaults;
    for (let i = 0; i <= 9; i++) {
      const paint = palette[i - 1];
      const row = make("div");
      const label = i === 0 ? (diving() ? "Headlamp" : "Heat") : paint?.label ?? "Empty";
      const equip = button(`${i} · ${label}`, () => {
        pendingSlot = i;
        close();
      });
      equip.disabled = i !== 0 && !paint;
      equip.setAttribute("aria-pressed", String(i === selected));
      if (paint) equip.prepend(swatch(paint));
      row.append(equip);
      if (i) {
        const edit = button(paint ? "Edit" : "Add", () => paintEditor(i, paint));
        edit.setAttribute("aria-label", `Edit paint slot ${i}`);
        row.append(edit);
      }
      grid.append(row);
    }
    dialog.append(grid);
    actions(closeButton());
  }
  function field(label, node) {
    const row = make("label", label, { class: "review-field" });
    row.append(node);
    return row;
  }
  function paintEditor(index, prior) {
    dialog.dataset.view = "paint-editor";
    view(`Edit paint · slot ${index}`);
    const paint = prior ?? { id: uuid(), label: "", spray: "", prompt: "", color: [255, 126, 141] };
    const name = make("input", undefined, { maxlength: 35, value: paint.label });
    const spray = make("input", undefined, { maxlength: 32, value: paint.spray });
    const prompt = make("textarea", paint.prompt, { rows: 4, maxlength: 4000 });
    const pigment = make("input", undefined, { type: "color", value: hex(paint.color) });
    const slot = make("select");
    for (let i = 1; i <= 9; i++) slot.append(make("option", String(i)));
    slot.value = String(index);
    const preview = make("p", "", { class: "paint-preview" });
    const refresh = () => { preview.textContent = spray.value || name.value; preview.style.color = pigment.value; };
    refresh();
    for (const node of [name, spray, pigment]) node.addEventListener("input", refresh);
    const status = make("p", "", { role: "status", class: "review-status" });
    const save = button("Save paint", async () => {
      const label = name.value.trim();
      if (!label) { status.textContent = "Give the paint a name."; tablet.focus(name); return; }
      const data = session;
      const palette = structuredClone(data.palette.length ? data.palette : defaults);
      const dest = Number(slot.value);
      palette[index - 1] = palette[dest - 1];
      palette[dest - 1] = {
        id: paint.id, label, spray: spray.value.trim(), prompt: prompt.value.trim(),
        color: pigment.value.slice(1).match(/../g).map(byte => parseInt(byte, 16)),
      };
      save.disabled = true;
      try {
        await saveReview(data.projectScope, "palette", { palette });
        data.palette = palette;
        pendingSlot = dest;
        selected = dest;
        repaint();
        paintsPanel();
      } catch {
        status.textContent = "Could not save the paint.";
        save.disabled = false;
      }
    });
    dialog.append(field("Name", name), field("Spray text", spray), field("Reusable prompt", prompt),
      field("Color", pigment), field("Slot", slot), preview, status);
    actions(button("Cancel", paintsPanel), save);
  }

  async function gitHistory(page = historyPage) {
    if (!session) return;
    const data = session;
    historyPage = typeof page === "number" ? page : historyPage;
    dialog.dataset.view = "git-history"; dialog.dataset.tablet = "";
    view("Git history");
    const selection = make("p", "", { class: "review-status" });
    const tools = make("div", undefined, { class: "dialog-actions" });
    const compare = button("Compare selected commits", async () => {
      if (!historySelection.before || !historySelection.after) return;
      compare.disabled = true; feedback.textContent = "Loading selected commits…";
      try { await onCompare(data.review.repository, historySelection.before, historySelection.after); }
      catch (error) { if (current()) { feedback.textContent = error.message; compare.disabled = false; } }
    });
    const feedback = make("p", "Loading commit history…", { role: "status" });
    const list = make("div", undefined, { class: "review-changed-files" });
    const current = () => session === data && dialog.open && dialog.dataset.view === "git-history" && list.isConnected;
    function selected() {
      selection.textContent = `Before ${historySelection.before?.slice(0, 8) ?? "—"} → After ${historySelection.after?.slice(0, 8) ?? "—"}`;
      compare.disabled = !historySelection.before || !historySelection.after;
    }
    selected(); tools.append(compare, button("Refresh", () => gitHistory()));
    if (historyPage > 1) tools.append(button("Newer", () => gitHistory(historyPage - 1)));
    dialog.append(selection, tools, feedback, list); show();
    try {
      const rows = await loadCommitHistory(data.review.repository, data.review.revision, historyPage, githubRequest);
      if (!current()) return;
      feedback.textContent = rows.length ? `Page ${historyPage}` : "No commits in this history.";
      for (const commit of rows) {
        const row = make("article");
        const show = button(`${commit.id.slice(0, 8)} · ${commit.date.slice(0, 10)} · ${commit.author}\n${commit.summary}`, async () => {
          list.replaceChildren(make("p", "Loading commit…"));
          const back = button("Back to log", () => gitHistory()); tools.append(back);
          try { const text = await loadCommitDetails(data.review.repository, commit.id, githubRequest);
            if (current()) list.replaceChildren(make("pre", text, { class: "plain-file-preview" }));
          } catch (error) { if (current()) list.replaceChildren(make("p", error.message)); }
        });
        const actions = make("div", undefined, { class: "dialog-actions" });
        for (const side of ["before", "after"]) actions.append(button(side === "before" ? "Before" : "After", () => { historySelection[side] = commit.id; selected(); }));
        row.append(show, actions); list.append(row);
      }
      if (rows.length === 100) tools.append(button("Older", () => gitHistory(historyPage + 1)));
    } catch (error) { if (current()) feedback.textContent = error.message; }
  }

  function tracePanel() {
    if (!session) return;
    dialog.dataset.view = "trace"; dialog.dataset.tablet = "";
    view("Paste a trace");
    dialog.append(make("p", "Paste a stack trace, OpenTelemetry OTLP JSON or Chrome Trace JSON. Source stays on this device."));
    const input = make("textarea", undefined, { "aria-label": "Stack trace", maxlength: "65536", rows: "8", spellcheck: "false" });
    input.value = traceText;
    input.addEventListener("input", () => { traceText = input.value; follow.disabled = true; if (group) group.disabled = true; });
    const status = make("p", tracePending ? "Building route…" : traceReport?.error ?? "", { role: "status" });
    const build = button("Build route", () => {
      traceText = input.value; tracePending = true;
      traceBuiltText = traceText;
      traceRequest = { action: "import", text: traceText };
      status.textContent = "Building route…";
      build.disabled = true; follow.disabled = true;
    });
    const follow = button("Follow trace", () => {
      tracePending = true; traceRequest = { action: "follow" };
      status.textContent = "Opening trace…"; build.disabled = true; follow.disabled = true;
    });
    build.disabled = tracePending;
    const frames = traceReport?.frames?.filter(frame => (frame.span?.group ?? "stack") === traceSelected) ?? [];
    follow.disabled = tracePending || traceText !== traceBuiltText || !frames.length || Boolean(traceReport?.error);
    dialog.append(input, status, make("p", "Up to 64 KiB and 128 frames or spans. Locations are checked against the selected snapshot when opened."));
    let group;
    if (traceReport?.groups?.length > 1) {
      dialog.append(make("p", "Choose one trace or thread to follow."));
      group = make("select", undefined, { "aria-label": "Trace or thread" });
      group.append(make("option", "Choose a trace or thread", { value: "" }));
      for (const entry of traceReport.groups) group.append(make("option", entry.label, { value: entry.id }));
      group.value = traceSelected ?? "";
      group.disabled = tracePending || traceText !== traceBuiltText;
      group.addEventListener("change", () => {
        tracePending = true; traceRequest = { action: "select", group: group.value };
        group.disabled = true; follow.disabled = true;
      });
      dialog.append(group);
    }
    actions(build, follow);
    if (traceReport) {
      dialog.append(make("p", traceReport.format), make("p", `${traceReport.frames.length} frames · ${traceReport.issues.length} unresolved locations · ${traceReport.ignored} ignored entries`));
      for (const frame of frames) {
        let label = `${frame.path}:${frame.line}${frame.suffix ? " (path suffix match)" : ""}`;
        if (frame.span) label = `${frame.span.name} · ${label}${frame.span.duration_ms == null ? "" : ` · ${frame.span.duration_ms.toFixed(3)} ms`}${frame.span.error ? " · error" : ""}${frame.span.parent ? ` · parent ${frame.span.parent}` : ""}`;
        dialog.append(make("p", label));
      }
      for (const issue of traceReport.issues) dialog.append(make("p", issue));
    }
    show(); tablet.focus(input);
  }

  function fileSearch() {
    if (!session) return;
    if (dialog.open && dialog.dataset.view === "file-search") {
      tablet.focus(dialog.querySelector('input[type="search"]'));
      return;
    }
    const data = session;
    const paths = (canvas.hasAttribute("data-changes-only") ? changedPaths(data.snapshot)
      : [...new Set([...Object.keys(data.snapshot.before), ...Object.keys(data.snapshot.after)])]).sort();
    dialog.dataset.view = "file-search"; dialog.dataset.tablet = "";
    view("Find a file");
    const input = make("input", undefined, { type: "search", placeholder: "File name or path", "aria-label": "File name or path", autocomplete: "off" });
    input.autofocus = true;
    const status = make("p", "", { role: "status" }), list = make("div", undefined, { class: "review-changed-files" });
    let matches = [];
    const choose = path => { fileDestination = path; close(); };
    function render() {
      const query = input.value.trim().toLowerCase();
      const rank = path => { const name = path.split("/").at(-1).toLowerCase(), value = path.toLowerCase();
        if (!query || name === query) return 0; if (name.startsWith(query)) return 1; if (name.includes(query)) return 2;
        if (value.includes(query)) return 3; let index = 0; for (const char of value) if (char === query[index]) index++; return index === query.length ? 4 : 5;
      };
      matches = paths.map(path => [rank(path), path]).filter(([score]) => score < 5).sort((a, b) => a[0] - b[0] || a[1].localeCompare(b[1])).map(([, path]) => path);
      status.textContent = matches.length ? `${matches.length} files${matches.length > 100 ? "; first 100 shown" : ""}` : "No matching files.";
      list.replaceChildren(...matches.slice(0, 100).map(path => button(path, () => choose(path))));
    }
    input.addEventListener("input", render);
    input.addEventListener("keydown", event => { if (event.key === "Enter" && matches[0]) { event.preventDefault(); choose(matches[0]); } else if (event.key === "ArrowDown") { event.preventDefault(); list.querySelector("button")?.focus(); } });
    dialog.append(input, status, list); render(); show(); tablet.focus(input);
  }

  function changedFiles() {
    if (!session) return;
    const data = session;
    const { before, after } = data.snapshot;
    const changed = changedPaths(data.snapshot);
    dialog.dataset.view = "changed-files";
    dialog.dataset.tablet = "";
    view("Changed files");
    const bar = make("div", undefined, { class: "dialog-actions" });
    const status = make("p", "Loading file dates…", { class: "review-status", role: "status" });
    const list = make("div", undefined, { class: "review-changed-files" });
    let viewedOnly = false;
    let viewed = new Set();
    let dates = {};
    const all = button("All changes", () => { viewedOnly = false; render(); });
    const seen = button("Viewed", () => { viewedOnly = true; render(); });
    bar.append(all, seen, button("Open canvas", () => onCanvas("changed")),
      button("Choose files for canvas", () => onCanvas("choose")));
    dialog.append(bar, status, list);
    show();

    const visible = () => session === data && dialog.open && dialog.dataset.view === "changed-files";
    function render() {
      all.setAttribute("aria-pressed", String(!viewedOnly));
      seen.setAttribute("aria-pressed", String(viewedOnly));
      const paths = changed.filter(path => !viewedOnly || viewed.has(path)).sort(
        (a, b) => (dates[b] ?? 0) - (dates[a] ?? 0) || a.localeCompare(b)
      );
      const fragment = document.createDocumentFragment();
      for (const path of paths) {
        const row = button(path, () => {
          fileDestination = path;
          close();
        });
        if (dates[path]) row.append(make("time", new Date(dates[path]).toISOString().slice(0, 10)));
        fragment.append(row);
      }
      if (!paths.length) fragment.append(make("p", viewedOnly ? "No changed files viewed." : "No changed files."));
      list.replaceChildren(fragment);
    }
    render();
    void readProgress(data.progressScope).then(values => {
      viewed = new Set(values.map(value => {
        const mark = JSON.parse(value);
        return mark.visited?.path ?? mark.reveal?.path ?? mark.read_document?.path ?? mark.reviewed;
      }).filter(Boolean));
      if (visible() && viewedOnly) render();
    }).catch(() => {});
    void data.loadChangeDates(changed).then(value => {
      dates = value;
      if (visible()) {
        status.textContent = Object.keys(dates).length ? "" : "Commit dates unavailable.";
        render();
      }
    }).catch(() => {
      if (visible()) status.textContent = "Commit dates unavailable.";
    });
  }

  function notesPanel() {
    dialog.dataset.view = "notes";
    const { review, pending, status, prior } = session;
    view(review.pull ? `Review · PR #${review.pull}` : "Review notes");
    if (review.pull && review.pullState !== "open") {
      dialog.append(make("p", review.pullState === "merged" ? "Merged pull request" : "Closed pull request"));
    }
    if (review.pull && !githubAccount()?.login) {
      dialog.append(make("p", "Sign in to create private review drafts."), signIn());
    } else if (!review.pull) {
      dialog.append(make("p", "Open a GitHub pull request to leave review notes."));
    }
    if (prior && prior.lastHead !== review.revision) {
      const query = new URLSearchParams({ repo: review.repository, ref: review.revision, base: prior.lastHead });
      dialog.append(make("p", "The PR has changed since your last review. Drafts keep their original lines."), link("Changes since your last review", "/?" + query));
    }
    if (review.pull && githubAccount()?.login) {
      const data = session;
      const resolved = make("input", undefined, { type: "checkbox" });
      resolved.checked = data.showResolved;
      const feedback = make("span", "", { role: "status" });
      const toggle = make("label", undefined, { class: "review-toggle" });
      toggle.append(resolved, "Show resolved threads");
      dialog.append(toggle, feedback);
      resolved.addEventListener("change", async () => {
        const showResolved = resolved.checked;
        resolved.disabled = true;
        try {
          await saveReview(data.scope, "thread-display", { showResolved });
          data.showResolved = showResolved;
          if (session === data) {
            repaint();
            if (dialog.open && dialog.dataset.view === "notes") {
              notesPanel();
              const checkbox = dialog.querySelector('input[type="checkbox"]'); if (checkbox) tablet.focus(checkbox);
            }
          }
        } catch {
          resolved.checked = data.showResolved;
          feedback.textContent = "Could not save this preference.";
        } finally {
          resolved.disabled = false;
        }
      });
    }
    if (status || pending) dialog.append(make("p", status || "Loading GitHub comments…", { role: "status" }));
    const list = make("div", undefined, { class: "review-list" });
    const entries = notes();
    const addressed = entries.filter(note => note.attention);
    if (addressed.length) dialog.append(button("Fly to next mention", () => {
      const index = addressed.findIndex(note => note.id === session.lastMention);
      const note = addressed[(index + 1) % addressed.length];
      session.lastMention = note.id;
      flyTo(note);
    }));
    for (const note of entries) {
      const caption = note.remote ? (note.attention ? "For you" : note.resolved ? "Resolved" : note.fileLevel ? "File comment" : note.original ? "Original location" : "GitHub thread") :
        note.published ? "Published" : current(note, review) ? "Draft" : "Earlier draft";
      const item = button(`${note.preset.label} · ${pathLabel(note)} · ${caption}`, () => note.remote ? threadPanel(note) : noteEditor(note));
      item.prepend(swatch(note.preset));
      list.append(item);
    }
    if (!entries.length && !pending) list.append(make("p", "No notes yet."));
    dialog.append(list);
    const controls = [];
    if (allowedDraft() && !pending) controls.unshift(button("Mark snapshot reviewed", async () => {
      const data = session;
      try {
        const { snapshot, progressScope } = data;
        const paths = new Set((await readProgress(progressScope)).map(json => {
          const { reveal, read_document, reviewed } = JSON.parse(json);
          return reveal?.path ?? read_document?.path ?? reviewed;
        }).filter(Boolean));
        const blobs = Object.fromEntries([...paths].map(path => [path, {
          before: snapshot.before[path]?.id ?? null, after: snapshot.after[path]?.id ?? null,
        }]));
        const checkpoint = { lastHead: review.revision, progressScope, blobs };
        await saveReview(data.scope, "lastHead", checkpoint);
        data.prior = checkpoint;
        close();
      } catch {
        data.status = "Could not save the review checkpoint.";
        notesPanel();
      }
    }));
    if (session.notes.length) controls.unshift(copyButton(session.notes));
    actions(...controls);
  }

  function flyTo(note) {
    destination = { anchor: note.anchor, revision: note.revision };
    close();
  }

  function copyButton(entries) {
    const control = button("Copy", async () => {
      try {
        const text = entries.map(note => `${pathLabel(note)}\n${note.text || note.preset.prompt || note.preset.label}`).join("\n\n");
        await navigator.clipboard.writeText(text);
        control.textContent = "Copied";
      } catch {
        control.textContent = "Could not copy";
      }
    });
    return control;
  }
  function threadPanel(note) {
    const data = session;
    dialog.dataset.view = "thread";
    view(pathLabel(note));
    dialog.append(make("p", note.fileLevel ? "File comment" : note.original ? "Original location · no current line anchor" : "GitHub thread"));
    if (note.resolved !== null) dialog.append(make("p", note.resolved ? "Resolved on GitHub" : "Open on GitHub"));
    for (const item of note.discussion) {
      const article = make("article");
      article.append(make("strong", item.user?.login ?? "Unknown author"),
        markdown(readCommentText(item.body).text));
      const href = safeDiscussion(item.html_url);
      if (href) article.append(link("View on GitHub", href));
      const personal = note.personal.find(comment => comment.id === item.id);
      if (personal) {
        const feedback = make("span", personal.viewed ? "Viewed on GitHub" : "", { role: "status" });
        const viewed = button("Mark as viewed", async () => {
          viewed.disabled = true;
          try {
            const response = await fetch(`/github/review/${data.review.repository}/${data.review.pull}/comments/${item.id}/eyes`, {
              method: "POST", signal: AbortSignal.timeout(15_000),
            });
            const result = await response.json();
            if (!response.ok || !result.viewed) {
              feedback.textContent = result.error || "GitHub did not confirm the reaction.";
              if (response.status === 401) feedback.append(" ", signIn());
              const install = safeDiscussion(githubAccount()?.installUrl);
              if (response.status === 403 && install) feedback.append(" ", link("Manage repository access", install));
              return;
            }
            personal.viewed = true;
            if (session === data) repaint();
            feedback.textContent = "Viewed on GitHub";
            viewed.remove();
          } catch {
            feedback.textContent = "GitHub did not confirm the reaction. Try again.";
          } finally {
            viewed.disabled = false;
          }
        });
        if (!personal.viewed) article.append(make("p", "Adds an eyes reaction on GitHub. The comment text stays unchanged."), viewed);
        article.append(" ", feedback);
      }
      dialog.append(article);
    }
    const prior = data.threadPaints.get(note.id);
    const palette = (data.palette.length ? data.palette : defaults).filter(Boolean);
    if (prior && !palette.some(paint => paint.id === prior.id)) palette.push(prior);
    const paint = make("select");
    paint.append(make("option", "Use thread color", { value: "" }));
    for (const preset of palette) paint.append(make("option", preset.label, { value: preset.id }));
    paint.value = prior?.id ?? "";
    const feedback = make("p", "", { role: "status", class: "review-status" });
    const save = button("Save local paint", async () => {
      const chosen = palette.find(preset => preset.id === paint.value);
      const key = "thread-paint-" + note.id;
      save.disabled = true;
      try {
        if (chosen) {
          await saveReview(data.scope, key, { threadId: note.id, paint: chosen });
          data.threadPaints.set(note.id, chosen);
        } else {
          await deleteReview(data.scope, key);
          data.threadPaints.delete(note.id);
        }
        if (session === data) {
          repaint();
          if (dialog.open && dialog.dataset.view === "thread") notesPanel();
        }
      } catch {
        feedback.textContent = "Could not save the local paint.";
      } finally {
        save.disabled = false;
      }
    });
    dialog.append(field("Local paint", paint), feedback);
    const travel = note.original || note.fileLevel ? [] : [button("Fly to line", () => flyTo(note))];
    actions(button("Back", notesPanel), ...travel, save);
  }

  function noteEditor(note) {
    dialog.dataset.view = "draft";
    view(pathLabel(note));
    if (note.published) dialog.append(link("Published on GitHub", note.published));
    if (!current(note, session.review)) dialog.append(make("p", "This draft belongs to an earlier revision."));
    const text = make("textarea", note.text, { rows: 5, maxlength: 6000, placeholder: "Add context for this range" });
    const code = make("textarea", note.replacement ?? "", {
      rows: 6, maxlength: replacementLimit, spellcheck: "false", placeholder: "Paste the code that should replace these lines",
    });
    let suggesting = typeof note.replacement === "string";
    const preset = make("select");
    const data = session;
    const palette = [note.preset, ...(data.palette.length ? data.palette : defaults).filter(p => p && p.id !== note.preset.id)];
    for (const paint of palette) preset.append(make("option", paint.label, { value: paint.id }));
    const chosen = () => palette.find(paint => paint.id === preset.value) ?? note.preset;
    const paintLabel = make("p", note.preset.label);
    paintLabel.prepend(swatch(note.preset));
    const paintField = field("Paint", preset);
    const textField = field("Comment", text);
    const preview = markdown(note.text);
    const codeField = field("Replacement code", code);
    const edit = button("Edit comment", () => {
      editing = true;
      refreshEditor();
      tablet.focus(text);
    });
    let editing = !note.published && (suggesting || !note.text.trim());
    const write = button("Write a comment", () => {
      editing = true;
      text.value = "";
      refreshEditor();
      tablet.focus(text);
    });
    const suggest = button("Suggest replacement", () => {
      suggesting = !suggesting;
      if (suggesting) editing = true;
      refreshEditor();
      if (suggesting) tablet.focus(code);
    });
    const canPublish = allowedDraft() && current(note, session.review) && !note.published;
    const canSuggest = canPublish && !note.anchor.before;
    dialog.append(paintLabel, paintField, preview, textField, codeField, edit, write, suggest);
    const status = make("p", "", { role: "status", class: "review-status" });
    dialog.append(status);
    const persist = async () => {
      const edited = { ...note, text: text.value, preset: chosen(), replacement: suggesting ? code.value : undefined };
      await saveReview(data.scope, note.id, edited);
      data.notes = data.notes.map(value => value.id === note.id ? edited : value);
      repaint();
      return edited;
    };
    const save = button("Save draft", async () => {
      try { await persist(); notesPanel(); } catch { status.textContent = "Could not save this draft."; }
    });
    const prepare = button("Publish to GitHub", async () => {
      if (busy) return;
      if (suggesting ? !code.value.length : !text.value.trim()) {
        status.textContent = suggesting ? "Enter the replacement code." : "Write the comment you want to publish.";
        tablet.focus(suggesting ? code : text);
        return;
      }
      if (suggesting) {
        try { publishPanel(await persist()); } catch { status.textContent = "Could not save this draft."; }
      } else {
        await submitNote(data, persist, status);
      }
    });
    function refreshEditor() {
      paintLabel.hidden = editing;
      paintField.hidden = note.published || !editing;
      preview.hidden = editing || !note.text.trim();
      textField.hidden = !editing;
      codeField.hidden = !suggesting;
      edit.hidden = editing || note.published;
      write.hidden = editing || note.published || !canPublish;
      suggest.hidden = !canSuggest;
      suggest.textContent = suggesting ? "Use plain comment" : "Suggest replacement";
      prepare.textContent = suggesting ? "Preview GitHub suggestion" : "Publish to GitHub";
    }
    refreshEditor();
    const remove = button("Delete draft", async () => {
      try {
        await deleteReview(data.scope, note.id);
        data.notes = data.notes.filter(value => value.id !== note.id);
        repaint();
        notesPanel();
      } catch { status.textContent = "Could not delete this draft."; }
    });
    if (note.published) {
      text.readOnly = code.readOnly = true;
      preset.disabled = true;
      actions(button("Back", notesPanel));
    } else {
      if (data.review.pull && !githubAccount()?.login && current(note, data.review)) dialog.append(signIn());
      actions(button("Back", notesPanel), copyButton([note]), remove, save, ...(canPublish ? [prepare] : []));
    }
  }

  function publishPanel(note, fileLevel = false) {
    dialog.dataset.view = "publish";
    const suggestion = typeof note.replacement === "string";
    view(suggestion ? "Publish a suggestion" : fileLevel ? "Publish a file comment" : "Publish a review comment");
    const data = session;
    dialog.append(make("p", `PR #${data.review.pull} · ${fileLevel ? note.anchor.path : pathLabel(note)}`));
    if (note.text.trim()) dialog.append(markdown(note.text.trim()));
    if (suggestion) dialog.append(make("pre", note.replacement, { class: "review-copy" }));
    const access = make("p", "Publishing requires write access to pull requests. ");
    const install = safeDiscussion(githubAccount()?.installUrl);
    if (install) access.append(link("Manage repository access", install));
    dialog.append(access);
    const status = make("p", "", { role: "status", class: "review-status" });
    dialog.append(status);
    const back = button("Back to draft", () => noteEditor(note));
    const submit = button("Publish to GitHub", () => submitNote(data, async () => note, status, fileLevel));
    actions(back, submit);
  }

  async function submitNote(data, persist, status, fileLevel = false) {
    if (busy) return;
    busy = true;
    const controls = [...dialog.querySelectorAll("button, textarea, select")];
    const enabled = controls.filter(control => !control.disabled);
    for (const control of enabled) control.disabled = true;
    status.textContent = "Saving draft…";
    try {
      let note;
      try { note = await persist(); }
      catch { status.textContent = "Could not save this draft."; return; }
      status.textContent = "Publishing…";
      const response = await fetch(`/github/review/${data.review.repository}/${data.review.pull}`, {
        method: "POST", headers: { "Content-Type": "application/json" }, signal: AbortSignal.timeout(40_000),
        body: JSON.stringify({ ...note.anchor, id: note.id, revision: note.revision, text: note.text,
          replacement: note.replacement, preset: note.preset.id, fileLevel }),
      });
      const result = await response.json();
      if (!response.ok) {
        status.textContent = result.error || "GitHub did not publish the comment.";
        if (result.fileLevel) status.append(" ", button("Preview file comment", () => publishPanel(note, true)));
        if (response.status === 401) status.append(" ", signIn());
        if (response.status === 403) {
          const install = safeDiscussion(githubAccount()?.installUrl);
          if (install) status.append(" ", link("Manage repository access", install));
        }
        return;
      }
      const published = safeDiscussion(result.comment?.html_url);
      if (!published) throw new Error("GitHub did not confirm the comment.");
      const updated = { ...note, published };
      data.notes = data.notes.map(value => value.id === note.id ? updated : value);
      repaint();
      await saveReview(data.scope, note.id, updated).catch(() => {
        data.status = "Published on GitHub. The browser could not save its local status.";
      });
      if (session === data) noteEditor(updated);
    } catch {
      status.textContent = "GitHub did not confirm the comment. Your draft is still saved; retry to check for it.";
    } finally {
      for (const control of enabled) control.disabled = false;
      busy = false;
    }
  }

  function panel(kind, palette, slot, id) {
    if (!session) return;
    defaults = JSON.parse(palette);
    selected = slot;
    const note = kind === '"aimed"' && notes().find(note => note.id === id);
    if (kind === '"paints"') {
      dialog.dataset.tablet = "";
      paintsPanel();
    } else {
      dialog.dataset.tablet = "";
      if (note?.remote) threadPanel(note);
      else if (note) noteEditor(note);
      else notesPanel();
    }
    show();
  }
  return {
    takeTraceRequest() { const request = traceRequest; traceRequest = undefined; return request ? JSON.stringify(request) : ""; },
    traceReply(value) {
      const result = JSON.parse(value);
      tracePending = false; traceReport = result.report; traceSelected = result.selected;
      if (result.following) close();
      else if (dialog.open && dialog.dataset.view === "trace") tracePanel();
    },
    selectTab, load, version: () => version, snapshot, add, panel, changedFiles, fileSearch, help, helpVisible: () => dialog.open && dialog.dataset.view === "help", previewMarkdown, dismissTablet,
    partyPanel, setPartyRenderer(render) { renderParty = render; },
    takeMarkdownRead() {
      const mark = markdownRead;
      markdownRead = undefined;
      return mark ? JSON.stringify(mark) : "";
    },
    id: uuid, canPaint: allowedDraft,
    takeReadingSettings() {
      if (!readingChanged) return "";
      readingChanged = false;
      return JSON.stringify(reading);
    },
    takeFileDestination() {
      const value = fileDestination;
      fileDestination = undefined;
      return value ?? "";
    },
    equipWater() { pendingSlot = 0; },
    takeSlot() { const value = pendingSlot; pendingSlot = -1; return value; },
    takeDestination() {
      const value = destination;
      destination = undefined;
      return value ? JSON.stringify(value) : "";
    },
    modal: () => Boolean(document.querySelector("dialog[open]")),
    tablet: tablet.raised,
    tabletVisible: tablet.visible,
    tabletView: () => tablet.view(), tabletAction: value => tablet.action(value),
    tabletLayout: value => tablet.layout(value), readTablet: () => tablet.inspect(),
    escapeTablet() { return tablet.back() || dismissTablet(); },
  };
}
