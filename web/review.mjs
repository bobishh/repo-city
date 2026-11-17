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

export function createReview({ canvas, release, onClose }) {
  const dialog = document.querySelector("#review-dialog");
  let session, version = 0, selected = 0, pendingSlot = -1, busy = false, destination, fileDestination;
  const repaint = () => { version++; };
  let defaults = [];

  dialog.addEventListener("close", () => {
    delete dialog.dataset.tablet;
    onClose();
  });
  dialog.addEventListener("cancel", event => { if (busy) event.preventDefault(); });
  for (const type of ["keydown", "keyup"]) dialog.addEventListener(type, event => event.stopPropagation());
  canvas.addEventListener("wheel", event => {
    if (event.metaKey && document.pointerLockElement === canvas) event.preventDefault();
  }, { passive: false });

  function view(title, tag = "h2") {
    dialog.replaceChildren(make(tag, title, { id: "review-title" }));
    return dialog;
  }
  function actions(...children) {
    const row = make("div", undefined, { class: "dialog-actions" });
    row.append(...children);
    dialog.append(row);
  }
  function show() {
    release();
    if (!dialog.open) dialog.showModal();
  }
  const close = () => dialog.close();
  const closeButton = () => button("Close", close);
  const signIn = () => make("a", "Sign in with GitHub", { href: githubSignIn() });

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
          const { reveal, reviewed } = JSON.parse(json);
          if (reveal) {
            const side = reveal.before ? "before" : "after";
            const old = prior.blobs?.[reveal.path]?.[side];
            if (old && old === snapshot[side][reveal.path]?.id) carried.push([json, true]);
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
    actions(closeButton());
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
        return mark.visited?.path ?? mark.reveal?.path ?? mark.reviewed;
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
    const controls = [closeButton()];
    if (allowedDraft() && !pending) controls.unshift(button("Mark snapshot reviewed", async () => {
      const data = session;
      try {
        const { snapshot, progressScope } = data;
        const paths = new Set((await readProgress(progressScope)).map(json => {
          const { reveal, reviewed } = JSON.parse(json);
          return reveal?.path ?? reviewed;
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
    if (session.notes.length) controls.unshift(button("Export drafts", () => exportPanel()));
    actions(...controls);
  }

  function flyTo(note) {
    destination = { anchor: note.anchor, revision: note.revision };
    close();
  }

  function exportPanel() {
    dialog.dataset.view = "export";
    view("Export review notes");
    const data = session;
    const options = data.notes.filter(note => !note.published).map(note => {
      const input = make("input", undefined, { type: "checkbox", checked: "" });
      dialog.append(field(`${note.preset.label} · ${pathLabel(note)}`, input));
      return [input, note];
    });
    const save = button("Download JSON", () => {
      const entries = options.filter(([input]) => input.checked).map(([, note]) => note);
      if (!entries.length) return;
      const { repository, pull, revision, baseline } = data.review;
      const url = URL.createObjectURL(new Blob([JSON.stringify({
        version: 1, repository, pull, prUrl: pull ? `https://github.com/${repository}/pull/${pull}` : null,
        revision, baseline, notes: entries,
      }, null, 2)], { type: "application/json" }));
      Object.assign(document.createElement("a"), { href: url, download: "repo-city-review.json" }).click();
      setTimeout(() => URL.revokeObjectURL(url), 60_000);
    });
    save.disabled = options.length === 0;
    for (const [input] of options) input.addEventListener("change", () => {
      save.disabled = !options.some(([candidate]) => candidate.checked);
    });
    actions(button("Back", notesPanel), save);
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
        make("p", readCommentText(item.body).text, { class: "review-copy" }));
      const href = safeDiscussion(item.html_url);
      if (href) article.append(link("View on GitHub", href));
      const personal = note.personal.find(comment => comment.id === item.id);
      if (personal) {
        const feedback = make("span", personal.viewed ? "Viewed on GitHub 👀" : "", { role: "status" });
        const viewed = button("Mark viewed on GitHub 👀", async () => {
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
            feedback.textContent = "Viewed on GitHub 👀";
            viewed.remove();
          } catch {
            feedback.textContent = "GitHub did not confirm the reaction. Try again.";
          } finally {
            viewed.disabled = false;
          }
        });
        if (!personal.viewed) article.append(" ", viewed);
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
    actions(button("Back", notesPanel), ...travel, save, closeButton());
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
    const preview = make("pre", note.text, { class: "review-copy" });
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
      actions(button("Back", notesPanel), closeButton());
    } else {
      if (data.review.pullState === "open" && !githubAccount()?.login && current(note, data.review)) dialog.append(signIn());
      actions(button("Back", notesPanel), remove, save, ...(canPublish ? [prepare] : []));
    }
  }

  function publishPanel(note, fileLevel = false) {
    dialog.dataset.view = "publish";
    const suggestion = typeof note.replacement === "string";
    view(suggestion ? "Publish a suggestion" : fileLevel ? "Publish a file comment" : "Publish a review comment");
    const data = session;
    dialog.append(make("p", `PR #${data.review.pull} · ${fileLevel ? note.anchor.path : pathLabel(note)}`));
    if (note.text.trim()) dialog.append(make("pre", note.text.trim(), { class: "review-copy" }));
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
      delete dialog.dataset.tablet;
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
    load, version: () => version, snapshot, add, panel, changedFiles, id: uuid, canPaint: allowedDraft,
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
    tablet: () => dialog.open && "tablet" in dialog.dataset,
  };
}
