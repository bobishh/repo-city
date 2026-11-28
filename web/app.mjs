import { loadGitHub, parsePullRequest } from "./github.mjs";
import { createSyntaxCache } from "./syntax.mjs";
import { createCityAudio } from "./audio.mjs";
import { focusRepositoryPicker, githubAccount, githubRequest, refreshRepositoryPicker } from "./github-picker.mjs";
import { setupPointer } from "./pointer.mjs";
import { createTouchControls } from "./touch.mjs";
import { createReview } from "./review.mjs";
import { readProgress, rememberExploration, writeProgress } from "./progress.mjs";
import { createReviewParty } from "./review-party-client.mjs";
import { mountReviewParty } from "./review-party-ui.mjs";

const engineBuild = document.documentElement.dataset.engineBuild;
const engineDirectory = engineBuild ? `./game/${engineBuild}/` : "./game/";
const form = document.querySelector("#repository-form");
const worldSelect = form.elements.world;
const defaultWorld = worldSelect.options[0].value;
const status = document.querySelector("#status");
const canvas = document.querySelector("#repo-city");
const blame = document.querySelector("#blame");
const cityRepository = document.querySelector("#city-repository");
const cityFile = document.querySelector("#city-file");
const cityProgress = document.querySelector("#city-progress");
const cityHint = document.querySelector("#city-hint");
const changeRoute = document.querySelector("#change-review-route");
const [changeCaption, changeStops] = changeRoute.children;
const cityCount = cityProgress.querySelector(".city-count");
const startScreen = document.querySelector("#start-screen");
const startControls = document.querySelector("#start-controls");
const dialog = document.querySelector("#repository-dialog");
const help = document.querySelector("#help");
const guideKey = "repo-city-guide-v1";
let tourCommand = 0;
let changesOnlyRequested = false;
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
let canvasRequest = "";
let world = defaultWorld;
const library = () => world === "repo-polis";
const diving = () => world === "repo-reef";
const audio = createCityAudio({ active: () => running && !dialog.open && !canvas.hidden });
const releasePointer = setupPointer(canvas, dialog);
const touch = createTouchControls({ canvas, onPad: () => review.changedFiles(), walking: library, diving });
const review = createReview({
  audio, canvas, release: releasePointer, onClose: focusCanvas, library, diving,
  onCanvas: request => { canvasRequest = request; },
  onCompare: (repository, before, after) => openRepository(repository, after, before, true),
  onTour(started) {
    tourCommand = ((tourCommand & ~1) + 2) | Number(started);
    if (started) review.equipWater();
    try { localStorage.setItem(guideKey, "seen"); } catch { /* Available for this session. */ }
  },
});
let partyDestination;
let boardChat, boardSketch, boardRoom;
const party = createReviewParty({
  async loadComparison(comparison) {
    const name = `${comparison.owner}/${comparison.repo}`;
    const current = active?.repository.review;
    if (current?.repository.toLowerCase() === name.toLowerCase() && current.revision === comparison.head_sha && current.baseline === comparison.base_sha) return;
    await openRepository(comparison.pr_number ? `https://github.com/${name}/pull/${comparison.pr_number}` : name,
      comparison.head_sha, comparison.base_sha, true, comparison);
    if (active?.repository.review.revision !== comparison.head_sha || (active?.repository.review.baseline ?? null) !== comparison.base_sha) {
      throw new Error("The pinned room comparison could not be loaded.");
    }
  },
  navigate(anchor) { partyDestination = anchor; review.dismissTablet(); focusCanvas(); },
});
function partyComparison() {
  const current = active?.repository.review;
  if (!current) return null;
  const [owner, repo] = current.repository.split("/");
  return { owner, repo, head_sha: current.revision, base_sha: current.baseline, layout_version: 1,
    ...(current.pull ? { pr_number: current.pull } : {}) };
}
review.setPartyRenderer(root => mountReviewParty(root, party, { comparison: partyComparison }));
window.addEventListener("blur", () => party.suspend());
document.addEventListener("visibilitychange", () => { if (document.hidden) party.suspend(); });
window.addEventListener("pagehide", () => party.dispose());
window.addEventListener("keydown", event => {
  if (event.code !== "F8" || event.repeat || event.target !== canvas || !party.state().voice) return;
  event.preventDefault(); party.talk(true);
});
window.addEventListener("keyup", event => { if (event.code === "F8") { event.preventDefault(); party.talk(false); } });
let sourceVersion = 0;
const loadingSource = "Loading source from GitHub…";
const canvasBlobConcurrency = 2;

function loadBlob(snapshot, sha) {
  const { repository, blobs } = snapshot;
  let entry = blobs.get(sha);
  if (entry) {
    blobs.delete(sha);
  } else {
    entry = { error: loadingSource, pending: true, promise: repository.blob(sha) };
    snapshot.pendingBlobs++;
    entry.promise.then(
      bytes => { entry.bytes = bytes; },
      error => { entry.error = error.message; },
    ).finally(() => {
      entry.pending = false;
      snapshot.pendingBlobs--;
      blobs.delete(sha);
      blobs.set(sha, entry);
      // Keep the four most recent results and every request still in flight.
      if (blobs.size - snapshot.pendingBlobs > 4) {
        for (const [key, item] of blobs) {
          if (!item.pending) { blobs.delete(key); break; }
        }
      }
      if (snapshot === active) sourceVersion++;
    });
  }
  blobs.set(sha, entry);
  return entry;
}
let repositoryVersion = 0;
let progressResetVersion = 0;
const parameters = new URLSearchParams(location.search);
const worldPickerEnabled = parameters.get("world-picker") === "1";
worldSelect.closest(".world-picker").hidden = !worldPickerEnabled;
const worldParam = parameters.get("world");
const requestedWorld = worldParam === "abyss" ? "repo-reef" : worldParam;
if (Array.from(worldSelect.options).some(option => option.value === requestedWorld)) {
  worldSelect.value = requestedWorld;
}
function nameOpenButton() {
  const option = worldSelect.selectedOptions[0];
  submit.textContent = option.dataset.openLabel ?? "Open world";
  document.documentElement.dataset.world = worldSelect.value;
  const artwork = option.dataset.cover ?? "./assets/repo-city-start.webp";
  const image = startScreen.querySelector("img");
  if (image.getAttribute("src") !== artwork) image.src = artwork;
  document.title = worldSelect.value === defaultWorld ? "Repo City Web" : `${option.label} · Repo City Web`;
}
worldSelect.addEventListener("change", nameOpenButton);
nameOpenButton();
for (const name of ["repo", "ref", "base"]) form.elements[name].value = parameters.get(name) ?? "";
if (parameters.get("pr")) form.elements.repo.value = parameters.get("pr");

function focusCanvas() {
  if (running && !document.querySelector("dialog[open]")) canvas.focus({ preventScroll: true });
}

canvas.addEventListener("wheel", event => {
  if (canvas.hasAttribute("data-diff-canvas") && (event.metaKey || event.ctrlKey)) event.preventDefault();
}, { passive: false });

window.addEventListener("keydown", event => {
  if (event.target === canvas && canvas.hasAttribute("data-diff-canvas")) {
    if (/^(Arrow|Page)/.test(event.code) || ["Tab", "Home", "End", "Space", "Escape"].includes(event.code) ||
        ((event.metaKey || event.ctrlKey) && ["Equal", "Minus", "NumpadAdd", "NumpadSubtract", "Digit0"].includes(event.code))) event.preventDefault();
    return;
  }
  if (event.target === canvas && canvas.hasAttribute("data-library-input")) return;
  if (event.target === canvas && canvas.hasAttribute("data-library-map")) {
    if (/^(Arrow|Page)/.test(event.code) || ["Tab", "Home", "End"].includes(event.code)) event.preventDefault();
    return;
  }
  const locked = document.pointerLockElement === canvas;
  const plain = !event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey;
  if (running && !library() && event.target === canvas && event.key === "/" &&
      !event.metaKey && !event.ctrlKey && !event.altKey) event.preventDefault();
  if (event.target === canvas && canvas.hasAttribute("data-library-reading")
      && event.code === "KeyF" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    return;
  }
  if (event.altKey && event.code === "Digit0" && !event.ctrlKey && !event.metaKey) {
    event.preventDefault(); event.stopPropagation(); review.dismissTablet(); logo.click(); return;
  }
  if (running && library() && event.target === canvas &&
      (/^(Arrow|Page)/.test(event.code) || ["Home", "End", "Space"].includes(event.code))) {
    event.preventDefault();
  }
  if (event.altKey && !event.ctrlKey && !event.metaKey && /^Digit[1-9]$/.test(event.code) && review.selectTab(Number(event.code.slice(-1)) - 1)) {
    event.preventDefault(); event.stopPropagation(); return;
  }
  if (event.code === "KeyP" && (event.metaKey || event.ctrlKey) && !event.altKey && running && (!dialog.open || review.tabletVisible())) {
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
      if (event.code === "KeyH") review.help();
      else review.changedFiles();
    }
    return;
  }
  if (review.tabletVisible() && (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", "PageDown", "Home", "End", "Space", "Enter", "Backspace", "F6"].includes(event.code) || ((event.metaKey || event.ctrlKey) && ["KeyA", "KeyC", "KeyF", "KeyS", "KeyV", "KeyX", "KeyY", "KeyZ"].includes(event.code)))) event.preventDefault();
  if (event.code !== "KeyF" || event.repeat || event.metaKey || event.ctrlKey ||
      (library() && canvas.hasAttribute("data-action-menu")) ||
      event.altKey || (!locked && (event.target !== canvas || (canvas.hasAttribute("data-pointer-ui") && !canvas.hasAttribute("data-overview")))) ||
      document.querySelector("dialog[open]") || !active?.canFilterChanges) return;
  event.preventDefault();
  changesOnlyRequested = !changesOnlyRequested;
}, true);

help.addEventListener("click", () => { if (running) review.help(); });
function offerFirstTour() {
  try { if (localStorage.getItem(guideKey)) return; }
  catch { /* The tour remains available when storage is disabled. */ }
  review.help();
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
dialog.addEventListener("close", () => {
  if (running && worldSelect.value !== world) {
    worldSelect.value = world;
    nameOpenButton();
  }
  audio.refresh();
  focusCanvas();
});
dialog.addEventListener("cancel", event => {
  if (reset.disabled) event.preventDefault();
});
dialog.addEventListener("keydown", event => event.stopPropagation());
dialog.addEventListener("keyup", event => event.stopPropagation());
refreshRepositoryPicker();


function showProgress(count, done, total) {
  count.querySelector(".city-value").textContent = total ? `${done}/${total}` : "—";
  count.lastElementChild.max = total || 1;
  count.lastElementChild.value = done;
}

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
  const canFilterChanges = Boolean(snapshot.baseline);
  const comparisonLabel = pull ? "PR" : base?.trim() ? "Compare" : snapshot.baseline ? "Last commit" : "Initial";
  active = { repository, blobs: new Map(), pendingBlobs: 0, blameFiles: new Map(), marks, progressError, canFilterChanges, comparisonLabel };
  canvasRequest = "";
  cityRepository.textContent = name;
  cityRepository.title = name;
  changeRoute.hidden = true;
  changesOnlyRequested = false;
  progressPanel.hidden = false;
  repositoryVersion++;
  blame.hidden = true;
  blame.removeAttribute("href");
}

function queryFor(repo, ref, base) {
  const pull = parsePullRequest(repo);
  const query = new URLSearchParams({ [pull ? "pr" : "repo"]: repo.trim() });
  if (!pull && ref?.trim()) query.set("ref", ref.trim());
  if (!pull && base?.trim()) query.set("base", base.trim());
  if (worldSelect.value !== defaultWorld) query.set("world", worldSelect.value);
  if (worldPickerEnabled) query.set("world-picker", "1");
  return query;
}

async function openRepository(repo, ref, base, exactBase = false, pinnedComparison) {
  if (submit.disabled) return;
  if (party.state().room && !pinnedComparison) {
    status.textContent = "Leave the party before opening a different comparison.";
    return;
  }
  const pull = parsePullRequest(repo);
  const nextWorld = worldSelect.value;
  const worldName = worldSelect.selectedOptions[0].label;
  const query = queryFor(repo, ref, base);
  if (running && world !== nextWorld) {
    const previous = new URLSearchParams(location.search);
    if (exactBase || (previous.get("exact") === "1" &&
        ["repo", "pr", "ref", "base"].every(key => previous.get(key) === query.get(key)))) {
      query.set("exact", "1");
    }
    location.search = query.toString();
    return;
  }
  if (exactBase) query.set("exact", "1");
  submit.disabled = true;
  worldSelect.disabled = true;
  form.setAttribute("aria-busy", "true");
  status.textContent = running ? "Loading repository…" : "Loading repository and browser engine…";
  try {
    const firstOpen = !running;
    if (firstOpen) {
      const [repository, engine] = await Promise.all([
        loadGitHub(repo, pull ? null : ref, pull ? null : base, githubRequest, { exactBase, pinnedComparison }),
        import(new URL("repo_city.js", new URL(engineDirectory, import.meta.url)).href),
      ]);
      const graphics = await navigator.gpu?.requestAdapter();
      if (!graphics) throw new Error("WebGPU is unavailable. Open Repo City Web in a browser with WebGPU and graphics acceleration.");
      await stageRepository(repository, ref, base);
      world = nextWorld;
      canvas.dataset.world = world;
      globalThis.repoCity = {
        worldSet: () => world,
        mobileScene: () => matchMedia("(pointer: coarse)").matches,
        libraryReading(path, first, last, status, face) {
          const position = first ? `, lines ${first}–${last}` : "";
          const notice = status ? `, ${status}` : "";
          canvas.setAttribute("aria-label", path ? `${worldName}, ${path}${position}${notice}` : worldName);
          canvas.toggleAttribute("data-library-reading", Boolean(path));
          if (path) canvas.dataset.libraryFace = face.toLowerCase();
          else canvas.removeAttribute("data-library-face");
        },
        libraryTarget(target, reticle) {
          canvas.toggleAttribute("data-library-reticle", reticle);
          if (target) canvas.dataset.libraryTarget = target;
          else canvas.removeAttribute("data-library-target");
        },
        libraryMap(title, files) {
          canvas.removeAttribute("data-library-travel");
          if (title) {
            releasePointer();
            canvas.dataset.libraryMap = title;
            canvas.dataset.libraryMapFiles = String(files);
          } else {
            canvas.removeAttribute("data-library-map");
            canvas.removeAttribute("data-library-map-files");
            canvas.removeAttribute("data-library-note-pins");
          }
        },
        libraryTravel(folder) {
          canvas.dataset.libraryTravel = folder;
        },
        libraryFlight(state) {
          if (state) canvas.dataset.libraryFlight = state;
          else canvas.removeAttribute("data-library-flight");
        },
        libraryMapHover(folder) {
          if (folder) canvas.dataset.libraryMapHover = folder;
          else canvas.removeAttribute("data-library-map-hover");
        },
        libraryNotePins(count) {
          canvas.dataset.libraryNotePins = String(count);
        },
        libraryInput(prompt) {
          canvas.toggleAttribute("data-library-input", Boolean(prompt));
          if (prompt) canvas.setAttribute("aria-description", prompt);
          else canvas.removeAttribute("aria-description");
        },
        partyUpdate(value) { party.update(JSON.parse(value)); },
        partyActive: () => Boolean(party.state().room),
        partyPeers: () => JSON.stringify(party.state().members.filter(member => member.id !== party.state().member?.id)),
        takePartyBoard() {
          const { chat, sketch, room } = party.state();
          if (chat === boardChat && sketch === boardSketch && room?.id === boardRoom) return "";
          boardChat = chat; boardSketch = sketch; boardRoom = room?.id;
          return JSON.stringify({ chat: chat.slice(-32), sketch });
        },
        takePartyDestination() { const destination = partyDestination; partyDestination = undefined; return destination ? JSON.stringify(destination) : ""; },
        tourCommand: () => tourCommand,
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
        comparisonLabel: () => active.comparisonLabel,
        takeChangesOnlyRequest() {
          const requested = changesOnlyRequested;
          changesOnlyRequested = false;
          return requested;
        },
        reviewModal: review.modal,
        reviewTablet: review.tablet,
        tabletView: review.tabletView,
        tabletAction: review.tabletAction,
        takeCanvasRequest() {
          const request = canvasRequest;
          canvasRequest = "";
          return request;
        },
        tabletLayout: review.tabletLayout,
        readTablet: review.readTablet,
        takeTraceRequest: review.takeTraceRequest,
        traceReply: review.traceReply,
        audio,
        audioActivity: audio.activity,
        previewMarkdown(path, before, start = 0, end = 6, edit = false) {
          const snapshot = active;
          const { repository } = snapshot;
          const blob = repository.file(path, before);
          const image = /\.(png|jpe?g|webp|gif|bmp|ico|tiff?|tga|p[bnpg]m|pam|qoi|dds|hdr|rgbe|svg)$/i.test(path);
          const video = /\.(mp4|m4v|webm|mov|ogv)$/i.test(path);
          const markdown = /\.(md|markdown|mdown|mkd)$/i.test(path);
          review.previewMarkdown(path, async () => {
            if (!blob || blob.mode === "120000") throw new Error("No readable source in this snapshot.");
            if (image || video) {
              if (blob.size > 16 * 1024 * 1024) throw new Error(`${video ? "Video" : "Image"} exceeds the 16 MiB preview limit.`);
              return { bytes: await repository.blob(blob.id), format: path.split(".").pop().toLowerCase() };
            }
            const bytes = await loadBlob(snapshot, blob.id).promise;
            if (bytes.includes(0)) throw new Error("Binary asset: metadata only");
            let text;
            try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
            catch { throw new Error("Non-UTF-8 asset: metadata only"); }
            let rows = Number(Boolean(text) && !text.endsWith("\n"));
            for (let index = text.indexOf("\n"); index !== -1; index = text.indexOf("\n", index + 1)) rows++;
            return { text, pageOffsets: engine.document_page_offsets, mark: { path, blob: blob.id, before, rows }, identity: {
              repository: repository.review.repository.toLowerCase(), revision: repository.review.revision,
              baseline: repository.review.baseline ?? null, path, blob: blob.id, before,
              range: { start: Math.max(0, Math.min(rows, start)), end: Math.max(0, Math.min(rows, end)) },
            } };
          }, image ? "image" : video ? "video" : markdown ? "markdown" : "text", edit);
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
        showCityHud(file, city, total, hint, routeJson) {
          cityFile.textContent = file;
          cityFile.title = file;
          showProgress(cityCount, city, total);
          cityHint.textContent = hint;
          cityHint.hidden = !hint;
          const route = JSON.parse(routeJson);
          changeRoute.hidden = !route;
          if (route) {
            if (changeCaption.textContent !== route.caption) changeCaption.textContent = route.caption;
            while (changeStops.children.length > route.stops.length) changeStops.lastElementChild.remove();
            route.stops.forEach((stop, index) => {
              const dot = changeStops.children[index] ?? changeStops.appendChild(document.createElement("span"));
              if (dot.dataset.stop !== stop) dot.dataset.stop = stop;
            });
          }
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
          if (canvas.hasAttribute("data-diff-canvas") && !active.blobs.has(sha) &&
              active.pendingBlobs >= canvasBlobConcurrency) throw loadingSource;
          const entry = loadBlob(active, sha);
          if (entry.bytes) return entry.bytes;
          throw entry.error;
        },
      };
      canvas.hidden = false;
      if (world !== defaultWorld) canvas.setAttribute("aria-label", worldName);
      await engine.default();
      running = true;
      document.documentElement.dataset.playing = "";
      startScreen.hidden = true;
      cityRepository.hidden = false;
      cityProgress.hidden = library();
      startControls.hidden = true;
      help.hidden = false;
    } else {
      await stageRepository(await loadGitHub(repo, pull ? null : ref, pull ? null : base, githubRequest, { exactBase, pinnedComparison }), ref, base);
    }
    history.replaceState(null, "", `?${query}`);
    if (exactBase) {
      form.elements.repo.value = repo;
      form.elements.ref.value = ref ?? "";
      form.elements.base.value = base ?? "";
      refreshRepositoryPicker();
    }
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
    if (firstOpen && world === defaultWorld) offerFirstTour();
    audio.refresh();
    focusCanvas();
  } catch (error) {
    status.textContent = error instanceof Error ? error.message : String(error);
    if (!running) canvas.hidden = true;
  } finally {
    submit.disabled = false;
    worldSelect.disabled = false;
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
}
if (initial && !parameters.has("picker")) openRepository(initial, parameters.get("ref"), parameters.get("base"), parameters.get("exact") === "1");
else if (parameters.get("party")) review.partyPanel();
