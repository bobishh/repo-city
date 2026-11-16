import { loadGitHub, parsePullRequest } from "./github.mjs";
import { createAnalytics } from "./analytics.mjs";
import { browserKey } from "./analytics-config.mjs";
import { createSyntaxCache } from "./syntax.mjs";

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
const submit = form.querySelector('button[type="submit"]');
const syntax = createSyntaxCache();
let active;
let running = false;
let sourceVersion = 0;
let repositoryVersion = 0;
const parameters = new URLSearchParams(location.search);
for (const name of ["repo", "ref", "base"]) form.elements[name].value = parameters.get(name) ?? "";
if (parameters.get("pr")) form.elements.repo.value = parameters.get("pr");

function toggleRevisionInputs() {
  const pull = Boolean(parsePullRequest(form.elements.repo.value));
  for (const name of ["ref", "base"]) form.elements[name].disabled = pull;
}

function focusCanvas() {
  if (running && !dialog.open) canvas.focus({ preventScroll: true });
}

logo.addEventListener("click", () => {
  if (!running) {
    form.elements.repo.focus();
    return;
  }
  if (document.pointerLockElement === canvas) document.exitPointerLock();
  close.hidden = false;
  if (form.parentElement !== dialog) dialog.append(form);
  if (!dialog.open) dialog.showModal();
  form.elements.repo.focus();
});
close.addEventListener("click", () => dialog.close());
dialog.addEventListener("close", focusCanvas);
dialog.addEventListener("keydown", event => event.stopPropagation());
dialog.addEventListener("keyup", event => event.stopPropagation());
form.elements.repo.addEventListener("input", toggleRevisionInputs);
toggleRevisionInputs();

canvas.addEventListener("keydown", event => {
  if (event.code === "Tab") {
    event.preventDefault();
    if (event.repeat) return;
    if (document.pointerLockElement === canvas) document.exitPointerLock();
    else canvas.requestPointerLock()?.catch(() => {});
  }
  if (event.code === "Escape" && document.pointerLockElement === canvas) {
    document.exitPointerLock();
  }
});

function stageRepository(repository) {
  active = { repository, blobs: new Map() };
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
      stageRepository(await loadGitHub(repo, pull ? null : ref, pull ? null : base));
    } else {
      const [repository, engine] = await Promise.all([
        loadGitHub(repo, pull ? null : ref, pull ? null : base),
        import("./game/repo_city.js"),
      ]);
      const graphics = await navigator.gpu?.requestAdapter();
      if (!graphics) throw new Error("WebGPU is unavailable. Open Repo City in a browser with WebGPU and graphics acceleration.");
      stageRepository(repository);
      globalThis.repoCity = {
        snapshot: () => active.repository.snapshot,
        repositoryVersion: () => repositoryVersion,
        sourceVersion: () => sourceVersion,
        syntaxVersion: syntax.version,
        syntaxSpans: syntax.spans,
        showBlame(path, before, line) {
          const href = active.repository.blameURL(path, before, line);
          if (href) blame.href = href;
          else blame.removeAttribute("href");
          blame.hidden = !href;
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

form.addEventListener("submit", event => {
  event.preventDefault();
  const { repo, ref, base } = form.elements;
  openRepository(repo.value, ref.value, base.value);
});

const initial = parameters.get("pr") || parameters.get("repo");
if (initial) openRepository(initial, parameters.get("ref"), parameters.get("base"));
