import { loadGitHub, parsePullRequest } from "./github.mjs";
import { createAnalytics } from "./analytics.mjs";
import { browserKey } from "./analytics-config.mjs";
import { createSyntaxCache } from "./syntax.mjs";
import { focusRepositoryPicker, githubRequest, refreshPickerFooter, refreshRepositoryPicker } from "./github-picker.mjs";
import { setupPointer } from "./pointer.mjs";
import { readProgress, writeProgress } from "./progress.mjs";

const analytics = createAnalytics(browserKey);
analytics.pageView();

const form = document.querySelector("#repository-form");
const status = document.querySelector("#status");
const canvas = document.querySelector("#repo-city");
const blame = document.querySelector("#blame");
const startScreen = document.querySelector("#start-screen");
const startControls = document.querySelector("#start-controls");
const dialog = document.querySelector("#repository-dialog");
const logo = document.querySelector("#logo");
const close = document.querySelector("#close");
const reset = document.querySelector("#reset-progress");
const progressPanel = document.querySelector("#comparison-progress");
const progressSummary = document.querySelector("#progress-summary");
const submit = document.querySelector("#open-city");
const syntax = createSyntaxCache();
const releasePointer = setupPointer(canvas, dialog);
let active;
let running = false;
let sourceVersion = 0;
let repositoryVersion = 0;
let progressResetVersion = 0;
const parameters = new URLSearchParams(location.search);
for (const name of ["repo", "ref", "base"]) form.elements[name].value = parameters.get(name) ?? "";
if (parameters.get("pr")) form.elements.repo.value = parameters.get("pr");

function focusCanvas() {
  if (running && !dialog.open) canvas.focus({ preventScroll: true });
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
  focusRepositoryPicker();
});
close.addEventListener("click", () => dialog.close());
dialog.addEventListener("close", focusCanvas);
dialog.addEventListener("cancel", event => {
  if (reset.disabled) event.preventDefault();
});
dialog.addEventListener("keydown", event => event.stopPropagation());
dialog.addEventListener("keyup", event => event.stopPropagation());
refreshRepositoryPicker();


async function stageRepository(repository) {
  const marks = await readProgress(repository.progressScope).catch(error => {
    console.warn("Could not load review progress", error);
    return [];
  });
  active = { repository, blobs: new Map(), blameFiles: new Map(), marks };
  progressPanel.hidden = false;
  refreshPickerFooter();
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

async function openRepository(repo, ref, base) {
  if (submit.disabled) return;
  const started = performance.now();
  const pull = parsePullRequest(repo);
  const comparing = Boolean(pull || base?.trim());
  submit.disabled = true;
  form.setAttribute("aria-busy", "true");
  status.textContent = running ? "Loading repository…" : "Loading repository and browser engine…";
  try {
    if (running) {
      await stageRepository(await loadGitHub(repo, pull ? null : ref, pull ? null : base, githubRequest));
    } else {
      const [repository, engine] = await Promise.all([
        loadGitHub(repo, pull ? null : ref, pull ? null : base, githubRequest),
        import("./game/repo_city.js"),
      ]);
      const graphics = await navigator.gpu?.requestAdapter();
      if (!graphics) throw new Error("WebGPU is unavailable. Open Repo City in a browser with WebGPU and graphics acceleration.");
      await stageRepository(repository);
      globalThis.repoCity = {
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
          progressSummary.textContent = `${reviewed} ${files} reviewed · ${revealed} ${lines} revealed`;
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
    }
    history.replaceState(null, "", `?${queryFor(repo, ref, base)}`);
    status.textContent = "";
    if (dialog.open) dialog.close();
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
if (parameters.has("github")) status.textContent = "GitHub sign-in did not complete.";
if (initial && !parameters.has("picker")) openRepository(initial, parameters.get("ref"), parameters.get("base"));
