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

export function createReview({ canvas, release, onClose, onTour, audio }) {
  const dialog = document.querySelector("#review-dialog");
  const tablet = createTablet(dialog, canvas);
  const settingsKey = "repo-city-reading-v1";
  let reading = { mode: "delayed", block_lines: 6, min_seconds: 3, max_seconds: 6 };
  try {
    if (localStorage.getItem("repo-city-wash-mode") === "immediate") reading.mode = "immediate";
    const previous = JSON.parse(localStorage.getItem(settingsKey));
    const stored = previous && { ...reading, ...previous };
    if (stored && !readingError(stored)) reading = stored;
  } catch { /* Use the built-in preferences. */ }
  let readingChanged = true;
  let session, version = 0, selected = 0, pendingSlot = -1, busy = false, destination, fileDestination;
  let renderMarkdown;
  let markdownRead;
  let readingEnd;
  const repaint = () => { version++; };
  let defaults = [];

  dialog.addEventListener("close", () => {
    if (dialog.open) return;
    readingEnd?.disconnect();
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
  for (const type of ["keydown", "keyup"]) dialog.addEventListener(type, event => event.stopPropagation());
  canvas.addEventListener("wheel", event => {
    if (event.metaKey && document.pointerLockElement === canvas) event.preventDefault();
  }, { passive: false });

  function view(title, tag = "h2") {
    readingEnd?.disconnect();
    const heading = make(tag, title, { id: "review-title" });
    dialog.replaceChildren(heading);
    if ("tablet" in dialog.dataset) {
      const bar = make("header", undefined, { class: "vibepad-bar" });
      const gear = button("⚙", settings);
      gear.className = "vibepad-settings";
      gear.setAttribute("aria-label", "Settings");
      gear.title = "Settings";
      gear.setAttribute("aria-pressed", String(dialog.dataset.view === "settings"));
      bar.append(heading, gear);
      const tabs = make("nav", undefined, { "aria-label": "VibePad", class: "vibepad-tabs" });
      for (const [label, page, open] of [
        ["Files", "changed-files", changedFiles],
        ["Comments", "notes", () => { notesPanel(); show(); }],
        ["Help", "help", help],
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
  const closeButton = () => button("Close", close);
  const signIn = () => make("a", "Sign in with GitHub", { href: githubSignIn() });

  function markdown(text, container = make("div", undefined, { class: "markdown-preview markdown-body" })) {
    if (renderMarkdown) container.innerHTML = renderMarkdown(text);
    else container.textContent = text;
    return container;
  }

  function previewMarkdown(path, load) {
    dialog.dataset.view = "markdown";
    dialog.dataset.tablet = "";
    view(path);
    const content = make("article", "Loading document…", { class: "markdown-preview markdown-body", "aria-live": "polite" });
    dialog.append(content);
    show();
    void load().then(({ text, mark }) => {
      if (!content.isConnected) return;
      if (!text.trim()) { content.textContent = "Empty document."; return; }
      markdown(text, content);
      const end = make("div", undefined, { class: "markdown-end", "aria-hidden": "true" });
      content.append(end);
      readingEnd = new IntersectionObserver(entries => {
        if (dialog.open && end.isConnected && entries.some(entry => entry.isIntersecting)) {
          markdownRead = mark;
          readingEnd.disconnect();
        }
      }, { root: dialog });
      readingEnd.observe(end);
    }).catch(error => {
      if (content.isConnected) content.textContent = error.message;
    });
  }

  function help() {
    dialog.dataset.view = "help";
    dialog.dataset.tablet = "";
    view("Take a quick tour");
    dialog.append(make("p", "Click the Repo City logo to open the repository picker."));
    const controls = make("dl", undefined, { class: "review-controls" });
    for (const [keys, purpose] of [
      ["WASD / Space / Ctrl", "Fly and change altitude"],
      ["E / M", "Fly to the aimed wall or switch overview"],
      ["Tab / P", "Browse changes, preview Markdown at an aimed building, or find a file"],
      ["F / G / B", "Filter changed files, open diff, or open blame"],
      ["Scroll", "Read along a wall or move between review stops"],
      ["0 / 1–9 / U", "Select water, select paint, or edit paints"],
      ["C / J", "Follow changed lines or open review comments"],
    ]) controls.append(make("dt", keys), make("dd", purpose));
    dialog.append(controls);
    const finish = started => { onTour(started); close(); };
    actions(button("Skip", () => finish(false)), button("Start tour", () => finish(true)));
    show();
  }

  function readingError(values) {
    if (!Number.isInteger(values.block_lines) || values.block_lines < 1 || values.block_lines > 4294967295) {
      return "Lines per wash block must be a positive whole number.";
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
    if (!available.supported) feedback.textContent = "This browser cannot synthesize city audio.";
    else if (available.error) feedback.textContent = available.error;
    else if (!available.speech) feedback.textContent = "Robot speech needs an installed local browser voice.";
    return section;
  }

  function settings() {
    dialog.dataset.view = "settings";
    dialog.dataset.tablet = "";
    view("Settings");
    dialog.append(soundSettings());
    const form = make("form", undefined, { class: "review-reading" });
    const mode = make("select");
    for (const value of ["delayed", "immediate"]) {
      mode.append(make("option", value === "delayed" ? "Delayed" : "Immediate", { value }));
    }
    mode.value = reading.mode;
    const block = make("input", undefined, { type: "number", min: "1", step: "1", value: reading.block_lines });
    const bound = value => make("input", undefined, { type: "text", inputmode: "decimal", value: value ?? "", placeholder: "No limit" });
    const min = bound(reading.min_seconds), max = bound(reading.max_seconds);
    const status = make("p", "", { class: "review-status", role: "status" });
    form.noValidate = true;
    form.append(field("Water", mode), field("Lines per wash block", block), field("Minimum delay (seconds)", min), field("Maximum delay (seconds)", max),
      make("p", "Block size and time limits are independent. With no limits, seconds = non-whitespace characters × 60 / (238 × 4.6). Wrapped rows count separately. Use 1 for individual lines or larger blocks for reading in chunks. The default is 6; it is a preset, not a measured optimum."),
      make("p", "Brysbaert’s 2019 estimate describes English nonfiction, not code comprehension. We use its length-adjusted rate as an approximation. Leave either delay blank to remove that limit."),
      status);
    const row = make("div", undefined, { class: "dialog-actions" });
    row.append(make("button", "Save settings", { type: "submit" }));
    form.append(row);
    form.addEventListener("submit", event => {
      event.preventDefault();
      const seconds = input => input.value.trim() === "" ? null : Number(input.value);
      const next = { mode: mode.value, block_lines: Number(block.value), min_seconds: seconds(min), max_seconds: seconds(max) };
      const error = readingError(next);
      if (error) { status.textContent = error; return; }
      reading = next;
      readingChanged = true;
      try {
        localStorage.setItem(settingsKey, JSON.stringify(reading));
        status.textContent = "Saved.";
      } catch {
        status.textContent = "Applied for this session. Browser storage is unavailable.";
      }
    });
    dialog.append(form);
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
  function allowedDraft() { return session?.review.pullState === "open" && Boolean(githubAccount()?.login); }
  function add(json) {
    if (!allowedDraft()) {
      if (session?.review.pullState === "open") return "Sign in with GitHub to leave a PR note.";
      return "Open an active pull request to leave a note in the browser.";
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
    dialog.append(make("p", "Pick a tool. Wash source first, then mark a range with a paint."));
    const grid = make("div", undefined, { class: "paint-slots" });
    const palette = session?.palette.length ? session.palette : defaults;
    for (let i = 0; i <= 9; i++) {
      const paint = palette[i - 1];
      const row = make("div");
      const equip = button(i === 0 ? "0 · Water" : `${i} · ${paint?.label ?? "Empty"}`, () => {
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
      if (!label) { status.textContent = "Give the paint a name."; name.focus(); return; }
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

  function changedFiles() {
    if (!session) return;
    const data = session;
    const { before, after } = data.snapshot;
    const changed = [...new Set([...Object.keys(before), ...Object.keys(after)])].filter(
      path => before[path]?.id !== after[path]?.id || before[path]?.mode !== after[path]?.mode
    );
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
    bar.append(all, seen);
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
    } else if (review.pull && !githubAccount()?.login) {
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
              dialog.querySelector('input[type="checkbox"]')?.focus();
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
      text.focus();
    });
    let editing = !note.published && (suggesting || !note.text.trim());
    const write = button("Write a comment", () => {
      editing = true;
      text.value = "";
      refreshEditor();
      text.focus();
    });
    const suggest = button("Suggest replacement", () => {
      suggesting = !suggesting;
      if (suggesting) editing = true;
      refreshEditor();
      if (suggesting) code.focus();
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
        (suggesting ? code : text).focus();
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
      if (data.review.pullState === "open" && !githubAccount()?.login && current(note, data.review)) dialog.append(signIn());
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
    load, version: () => version, snapshot, add, panel, changedFiles, help, previewMarkdown, dismissTablet,
    setMarkdown(renderer) { renderMarkdown = renderer; },
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
    tabletPose: (corners, width, height) => tablet.project(corners, width, height),
  };
}
