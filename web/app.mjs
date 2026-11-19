import { loadGitHub, parsePullRequest } from "./github.mjs";
import { createAnalytics } from "./analytics.mjs";
import { browserKey } from "./analytics-config.mjs";
import { createSyntaxCache } from "./syntax.mjs";
import { createCityAudio } from "./audio.mjs";
import { focusRepositoryPicker, githubAccount, githubReady, githubRequest, refreshRepositoryPicker } from "./github-picker.mjs";
import { setupPointer } from "./pointer.mjs";
import { createTouchControls } from "./touch.mjs";
import { createReview } from "./review.mjs";
import { readProgress, rememberExploration, writeProgress } from "./progress.mjs";

const analytics = createAnalytics(browserKey);
analytics.pageView();

const form = document.querySelector("#repository-form");
const status = document.querySelector("#status");
const canvas = document.querySelector("#repo-city");
const blame = document.querySelector("#blame");
const startScreen = document.querySelector("#start-screen");
const startControls = document.querySelector("#start-controls");
const dialog = document.querySelector("#repository-dialog");
const help = document.querySelector("#help");
const guideKey = "repo-city-guide-v1";
let tourCommand = 0;
let tourEntry = null;
let changesOnlyRequested = false;
let activeTourEntry = null;
const logo = document.querySelector("#logo");
const close = document.querySelector("#close");
const reset = document.querySelector("#reset-progress");
const progressPanel = document.querySelector("#comparison-progress");
const progressSummary = document.querySelector("#progress-summary");
const progressWarning = document.querySelector("#progress-warning");
progressWarning.querySelector("button").addEventListener("click", () => { progressWarning.hidden = true; });
const submit = document.querySelector("#open-city");
const syntax = createSyntaxCache();
let active;
let running = false;
const audio = createCityAudio({ active: () => running && !dialog.open && !canvas.hidden });
const releasePointer = setupPointer(canvas, dialog);
const touch = createTouchControls({ canvas, onPad: () => review.changedFiles() });
const review = createReview({
  audio, canvas, release: releasePointer, onClose: focusCanvas,
  onCompare: (repository, before, after) => openRepository(repository, after, before, true),
  onTour(started) {
    tourCommand = ((tourCommand & ~1) + 2) | Number(started);
    activeTourEntry = started ? (tourEntry ?? "help") : null;
    analytics.tour(started ? "start" : "skip", tourEntry ?? "help");
    if (started) review.equipWater();
    try { localStorage.setItem(guideKey, "seen"); } catch { /* Available for this session. */ }
  },
});
let sourceVersion = 0;
let repositoryVersion = 0;
let progressResetVersion = 0;
const parameters = new URLSearchParams(location.search);
for (const name of ["repo", "ref", "base"]) form.elements[name].value = parameters.get(name) ?? "";
if (parameters.get("pr")) form.elements.repo.value = parameters.get("pr");

function focusCanvas() {
  if (running && !document.querySelector("dialog[open]")) canvas.focus({ preventScroll: true });
}

window.addEventListener("keydown", event => {
  const locked = document.pointerLockElement === canvas;
  const plain = !event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey;
  if (event.altKey && event.code === "Digit0" && !event.ctrlKey && !event.metaKey) {
    event.preventDefault(); event.stopPropagation(); review.dismissTablet(); logo.click(); return;
  }
  if (event.altKey && !event.ctrlKey && !event.metaKey && /^Digit[1-9]$/.test(event.code) && review.selectTab(Number(event.code.slice(-1)) - 1)) {
    event.preventDefault(); event.stopPropagation(); return;
  }
  if (event.code === "KeyP" && (event.metaKey || event.ctrlKey) && !event.altKey && running && !dialog.open) {
    event.preventDefault(); event.stopPropagation(); if (!event.repeat) review.fileSearch(); return;
  }
  if (event.code === "KeyH" && plain && review.helpVisible() && !/^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName)) {
    event.preventDefault(); event.stopPropagation();
    if (!event.repeat) review.dismissTablet();
    return;
  }
  if (event.code === "Escape" && review.tabletVisible()) {
    event.preventDefault(); event.stopPropagation(); review.escapeTablet(); return;
  }
  if (event.code === "Tab" && !plain && review.tabletVisible()) event.preventDefault();
  if (event.code === "Tab" && plain && review.tabletVisible()) {
    event.preventDefault();
    event.stopPropagation();
    if (!event.repeat) review.dismissTablet();
    return;
  }
  if (["Tab", "KeyH"].includes(event.code) && plain && running && (locked || event.target === canvas) &&
      !canvas.hasAttribute("data-pointer-ui") && !canvas.hasAttribute("data-action-menu") &&
      !document.querySelector("dialog[open]")) {
    event.preventDefault();
    event.stopPropagation();
    if (!event.repeat) {
      if (event.code === "KeyH") openGuide("help");
      else review.changedFiles();
    }
    return;
  }
  if (review.tabletVisible() && (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", "PageDown", "Home", "End", "Space", "Enter", "Backspace"].includes(event.code) || ((event.metaKey || event.ctrlKey) && ["KeyA", "KeyC", "KeyV", "KeyX", "KeyZ"].includes(event.code)))) event.preventDefault();
  if (event.code !== "KeyF" || event.repeat || event.metaKey || event.ctrlKey ||
      event.altKey || (!locked && (event.target !== canvas || canvas.hasAttribute("data-pointer-ui"))) ||
      document.querySelector("dialog[open]") || !active?.canFilterChanges) return;
  event.preventDefault();
  changesOnlyRequested = !changesOnlyRequested;
}, true);

function openGuide(entry) {
  tourEntry = entry;
  review.help();
}
help.addEventListener("click", () => { if (running) openGuide("help"); });
function offerFirstTour() {
  try { if (localStorage.getItem(guideKey)) return; }
  catch { /* The tour remains available when storage is disabled. */ }
  openGuide("first_run");
}

logo.addEventListener("click", () => {
  if (!running) {
    focusRepositoryPicker();
    return;
  }
  releasePointer();
  close.hidden = false;
  if (form.parentElement !== dialog) dialog.append(form);
  if (!dialog.open) dialog.showModal();
  audio.refresh();
  focusRepositoryPicker();
});
close.addEventListener("click", () => dialog.close());
dialog.addEventListener("close", () => { audio.refresh(); focusCanvas(); });
dialog.addEventListener("cancel", event => {
  if (reset.disabled) event.preventDefault();
});
dialog.addEventListener("keydown", event => event.stopPropagation());
dialog.addEventListener("keyup", event => event.stopPropagation());
refreshRepositoryPicker();


async function stageRepository(repository, ref, base) {
  await review.load(repository);
  const { review: { repository: name, pull }, progressScope } = repository;
  const snapshot = JSON.parse(repository.snapshot);
  if (pull || !/^(?:[a-f\d]{40}|[a-f\d]{64})$/i.test(ref?.trim())) {
    const account = githubAccount()?.login ?? null;
    const track = JSON.stringify(["exploration", account, name.toLowerCase(),
      pull ? ["pr", pull] : ["ref", ref?.trim() || null, base?.trim() || null]]);
    await rememberExploration(track, progressScope, snapshot).catch(error => {
      console.warn("Could not carry exploration progress", error);
    });
  }
  let progressError;
  const marks = await readProgress(repository.progressScope).catch(error => {
    console.warn("Could not load review progress", error);
    progressError = error;
    return [];
  });
  const { before, after } = snapshot;
  const canFilterChanges = Boolean(pull) && (Object.keys(before).length !== Object.keys(after).length ||
    Object.entries(before).some(([path, blob]) => after[path]?.id !== blob.id || after[path]?.mode !== blob.mode));
  active = { repository, blobs: new Map(), blameFiles: new Map(), marks, progressError, canFilterChanges };
  changesOnlyRequested = false;
  progressPanel.hidden = false;
  repositoryVersion++;
  blame.hidden = true;
  blame.removeAttribute("href");
}

function queryFor(repo, ref, base) {
  if (parsePullRequest(repo)) return new URLSearchParams({ pr: repo.trim() });
  const query = new URLSearchParams({ repo: repo.trim() });
  if (ref?.trim()) query.set("ref", ref.trim());
  if (base?.trim()) query.set("base", base.trim());
  return query;
}

async function openRepository(repo, ref, base, exactBase = false) {
  if (submit.disabled) return;
  const started = performance.now();
  const pull = parsePullRequest(repo);
  const comparing = Boolean(pull || base?.trim());
  submit.disabled = true;
  form.setAttribute("aria-busy", "true");
  status.textContent = running ? "Loading repository…" : "Loading repository and browser engine…";
  try {
    const firstOpen = !running;
    if (firstOpen) {
      const [repository, engine] = await Promise.all([
        loadGitHub(repo, pull ? null : ref, pull ? null : base, githubRequest, { exactBase }),
        import("./game/repo_city.js"),
      ]);
      const graphics = await navigator.gpu?.requestAdapter();
      if (!graphics) throw new Error("WebGPU is unavailable. Open Repo City in a browser with WebGPU and graphics acceleration.");
      await stageRepository(repository, ref, base);
      globalThis.repoCity = {
        tourCommand: () => tourCommand,
        tourCompleted() {
          if (!activeTourEntry) return;
          analytics.tour("complete", activeTourEntry);
          activeTourEntry = null;
        },
        reviewVersion: review.version,
        reviewRead: review.snapshot,
        reviewSave: review.add,
        reviewPanel: review.panel,
        reviewSlot: review.takeSlot,
        reviewDestination: review.takeDestination,
        takeFileDestination: review.takeFileDestination,
        takeTouchInput: touch.take,
        openFileSearch: review.fileSearch,
        canFilterChanges: () => active.canFilterChanges,
        takeChangesOnlyRequest() {
          const requested = changesOnlyRequested;
          changesOnlyRequested = false;
          return requested;
        },
        reviewModal: review.modal,
        reviewTablet: review.tablet,
        tabletView: review.tabletView,
        tabletAction: review.tabletAction,
        tabletLayout: review.tabletLayout,
        readTablet: review.readTablet,
        takeTraceRequest: review.takeTraceRequest,
        traceReply: review.traceReply,
        audio,
        audioActivity: audio.activity,
        previewMarkdown(path, before) {
          const { repository } = active;
          const snapshot = JSON.parse(repository.snapshot);
          const blob = snapshot[before ? "before" : "after"][path];
          const image = /\.(png|jpe?g|webp)$/i.test(path);
          const markdown = /\.(md|markdown|mdown|mkd)$/i.test(path);
          review.previewMarkdown(path, async () => {
            if (!blob || blob.mode === "120000") throw new Error("No readable source in this snapshot.");
            if (image) {
              if (blob.size > 16 * 1024 * 1024) throw new Error("Image exceeds the 16 MiB preview limit.");
              return { bytes: await repository.blob(blob.id), format: path.split(".").pop().toLowerCase() };
            }
            if (blob.size > 1024 * 1024) throw new Error("Source exceeds the 1 MiB inspection limit.");
            const bytes = await repository.blob(blob.id);
            if (bytes.includes(0)) throw new Error("Binary asset: metadata only");
            let text;
            try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
            catch { throw new Error("Non-UTF-8 asset: metadata only"); }
            const rows = text.split("\n").length - Number(!text || text.endsWith("\n"));
            return { text, mark: { path, blob: blob.id, before, rows } };
          }, image ? "image" : markdown ? "markdown" : "text");
        },
        takeMarkdownRead: review.takeMarkdownRead,
        takeReadingSettings: review.takeReadingSettings,
        reviewCanPaint: review.canPaint,
        reviewId: review.id,
        snapshot: () => active.repository.snapshot,
        repositoryVersion: () => repositoryVersion,
        sourceVersion: () => sourceVersion,
        syntaxVersion: syntax.version,
        syntaxSpans: syntax.spans,
        progressScope: () => active.repository.progressScope,
        progressMarks: () => JSON.stringify(active.marks),
        progressResetVersion: () => progressResetVersion,
        reportProgress(reviewed, revealed) {
          const files = reviewed === 1 ? "file" : "files";
          const lines = revealed === 1 ? "line" : "lines";
          progressSummary.textContent = active.progressError ? "Saved progress is unavailable." :
            `${reviewed} ${files} reviewed · ${revealed} ${lines} revealed`;
        },
        writeProgress(scope, json) {
          writeProgress(scope, JSON.parse(json)).catch(error => {
            console.warn("Could not save review progress", error);
          });
        },
        showBlame(path, before, line) {
          const href = active.repository.blameURL(path, before, line);
          if (href) blame.href = href;
          else blame.removeAttribute("href");
          blame.hidden = !href;
        },
        readBlame(path, before, lineCount) {
          const { repository, blameFiles } = active;
          const key = JSON.stringify([path, before, lineCount]);
          if (!blameFiles.has(key)) {
            if (blameFiles.size >= 24) blameFiles.delete(blameFiles.keys().next().value);
            const entry = { data: "" };
            blameFiles.set(key, entry);
            repository.blame(path, before, lineCount)
              .then(result => { entry.data = JSON.stringify(result); })
              .catch(() => { entry.data = '{"error":"unavailable"}'; });
          }
          return blameFiles.get(key).data;
        },
        blob(sha) {
          const { repository, blobs } = active;
          if (!blobs.has(sha)) {
            const entry = { error: "Loading source from GitHub…" };
            blobs.set(sha, entry);
            repository.blob(sha).then(bytes => { entry.bytes = bytes; })
              .catch(error => { entry.error = error.message; })
              .finally(() => { if (blobs === active.blobs) sourceVersion++; });
          }
          const entry = blobs.get(sha);
          if (entry.bytes) return entry.bytes;
          throw entry.error;
        },
      };
      canvas.hidden = false;
      await engine.default();
      running = true;
      document.documentElement.dataset.playing = "";
      startScreen.hidden = true;
      startControls.hidden = true;
      help.hidden = false;
    } else {
      await stageRepository(await loadGitHub(repo, pull ? null : ref, pull ? null : base, githubRequest, { exactBase }), ref, base);
    }
    const query = queryFor(repo, ref, base);
    if (exactBase) query.set("exact", "1");
    history.replaceState(null, "", `?${query}`);
    if (exactBase) review.dismissTablet();
    status.textContent = "";
    if (dialog.open) dialog.close();
    if (active.progressError) {
      progressWarning.querySelector("[role=status]").textContent =
        active.progressError.message || "Saved progress is unavailable.";
      progressWarning.hidden = false;
    } else {
      progressWarning.hidden = true;
    }
    if (firstOpen) offerFirstTour();
    audio.refresh();
    focusCanvas();
    analytics.repositoryOpened(true, performance.now() - started, comparing);
  } catch (error) {
    status.textContent = error instanceof Error ? error.message : String(error);
    if (!running) canvas.hidden = true;
    analytics.repositoryOpened(false, performance.now() - started, comparing);
  } finally {
    submit.disabled = false;
    form.removeAttribute("aria-busy");
  }
}

reset.addEventListener("click", async () => {
  if (submit.disabled || !active ||
      !window.confirm("Reset revealed lines and reviewed files for this comparison?")) return;
  reset.disabled = true;
  close.disabled = true;
  submit.disabled = true;
  try {
    await writeProgress(active.repository.progressScope, null);
    active.marks = [];
    progressResetVersion++;
    status.textContent = "Progress reset for this comparison.";
  } catch (error) {
    status.textContent = "Could not reset progress.";
    console.warn("Could not reset review progress", error);
  } finally {
    reset.disabled = false;
    close.disabled = false;
    submit.disabled = false;
  }
});

form.addEventListener("submit", event => {
  event.preventDefault();
  const { repo, ref, base } = form.elements;
  openRepository(repo.value, ref.value, base.value);
});

const initial = parameters.get("pr") || parameters.get("repo");
if (parameters.get("github") === "failed") status.textContent = "GitHub sign-in did not complete.";
if (parameters.get("github") === "connected") {
  const clean = new URL(location.href);
  clean.searchParams.delete("github");
  history.replaceState(null, "", clean);
  void githubReady().then(() => { if (githubAccount()?.login) analytics.githubConnected(); });
}
if (initial && !parameters.has("picker")) openRepository(initial, parameters.get("ref"), parameters.get("base"), parameters.get("exact") === "1");
