import { loadGitHub, parsePullRequest } from "./github.mjs";
import { createAnalytics } from "./analytics.mjs";
import { browserKey } from "./analytics-config.mjs";

const analytics = createAnalytics(browserKey);
analytics.pageView();

const form = document.querySelector("form");
const status = document.querySelector("#status");
const canvas = document.querySelector("#repo-city");
const startScreen = document.querySelector("#start-screen");
const submit = form.querySelector("button");
const parameters = new URLSearchParams(location.search);
for (const name of ["repo", "ref", "base"]) form.elements[name].value = parameters.get(name) ?? "";
if (parameters.get("pr")) form.elements.repo.value = parameters.get("pr");

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

async function openRepository(repo, ref, base) {
  const started = performance.now();
  const comparing = Boolean(base?.trim() || parsePullRequest(repo));
  submit.disabled = true;
  status.textContent = "Loading repository and browser engine…";
  try {
    const [repository, engine] = await Promise.all([
      loadGitHub(repo, ref, base),
      import("./game/repo_city.js"),
    ]);
    const graphics = await navigator.gpu?.requestAdapter();
    if (!graphics) throw new Error("WebGPU is unavailable. Open Repo City in a browser with WebGPU and graphics acceleration.");
    let version = 0;
    const blobs = new Map();
    globalThis.repoCity = {
      snapshot: () => repository.snapshot,
      sourceVersion: () => version,
      blob(sha) {
        if (!blobs.has(sha)) {
          const entry = { error: "Loading source from GitHub…" };
          blobs.set(sha, entry);
          repository.blob(sha).then(bytes => { entry.bytes = bytes; })
            .catch(error => { entry.error = error.message; })
            .finally(() => { version += 1; });
        }
        const entry = blobs.get(sha);
        if (entry.bytes) return entry.bytes;
        throw entry.error;
      },
    };
    canvas.hidden = false;
    await engine.default();
    startScreen.hidden = true;
    status.textContent = "";
    form.hidden = true;
    canvas.focus();
    analytics.repositoryOpened(true, performance.now() - started, comparing);
  } catch (error) {
    status.textContent = error instanceof Error ? error.message : String(error);
    canvas.hidden = true;
    analytics.repositoryOpened(false, performance.now() - started, comparing);
  } finally {
    submit.disabled = false;
  }
}

form.addEventListener("submit", event => {
  event.preventDefault();
  const query = new URLSearchParams(new FormData(form));
  if (globalThis.repoCity) location.search = query;
  else {
    history.replaceState(null, "", `?${query}`);
    openRepository(query.get("repo"), query.get("ref"), query.get("base"));
  }
});

document.querySelector("#change").addEventListener("click", () => {
  form.hidden = !form.hidden;
  if (!form.hidden) form.elements.repo.focus();
});

const initial = parameters.get("pr") || parameters.get("repo");
if (initial) openRepository(initial, parameters.get("ref"), parameters.get("base"));
