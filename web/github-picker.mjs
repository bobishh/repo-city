import { loadLibrary, loadRevisions, parsePullRequest, parseRepository } from "./github.mjs";

const publicAPI = "https://api.github.com";
const account = document.querySelector("#github-account");
const loginButton = document.querySelector("#github-login");
const logout = document.querySelector("#github-logout");
const install = document.querySelector("#github-install");
const library = document.querySelector("#github-library");
const group = document.querySelector("#repository-group");
const choices = document.querySelector("#repository-choice");
const form = document.querySelector("#repository-form");
const repo = form.elements.repo;
const refs = document.querySelector("#ref-choices");
const revisionOptions = document.querySelector("#revision-choices");
const revisionHint = document.querySelector("#revision-hint");
let session;
let libraryData, timer, latest;
const revisionCache = new Map();

export async function githubRequest(url, options) {
  await ready;
  if (session?.login && url.startsWith(publicAPI + "/")) {
    const path = url.slice(publicAPI.length);
    const response = await fetch("/github/api" + path, options);
    if (response.status === 401) {
      session = { ...session, login: null };
      paintAccount();
    }
    if (!/^\/(repos|users)\//.test(path) || ![401, 403, 404].includes(response.status)) return response;
  }
  return fetch(url, options);
}

const ready = fetch("/github/session", { cache: "no-store", signal: AbortSignal.timeout(2500) })
  .then(response => response.ok ? response.json() : null)
  .then(result => {
    session = result;
    paintAccount();
    if (session?.login) void fillLibrary();
  }).catch(() => {});

function paintAccount() {
  if (!session?.enabled) return;
  const { login, installUrl } = session;
  account.hidden = false;
  loginButton.hidden = Boolean(login);
  logout.hidden = !login;
  library.hidden = !login;
  install.hidden = !login || !installUrl;
  if (installUrl) install.href = installUrl;
  document.querySelector("#github-user").textContent = login ?? "";
}

function renderLibrary() {
  const items = libraryData?.[group.value] ?? [];
  const label = libraryData ? "Choose a repository" : "Loading repositories…";
  choices.replaceChildren(new Option(items.length ? label : (libraryData ? "No repositories in this list" : label), ""));
  for (const item of items) {
    choices.add(new Option(item.full_name + (item.private ? " · Private" : ""), item.full_name));
  }
  choices.disabled = !items.length;
}

async function fillLibrary() {
  renderLibrary();
  try {
    libraryData = await loadLibrary(session.login, githubRequest);
    renderLibrary();
    if (libraryData.partial) document.querySelector("#library-status").textContent = "Some GitHub lists are unavailable.";
  } catch {
    document.querySelector("#library-status").textContent = "Could not load your GitHub lists.";
  }
}

loginButton.addEventListener("click", () => {
  const resume = new URLSearchParams(location.search);
  if (repo.value.trim()) {
    for (const field of ["repo", "ref", "base", "pr"]) resume.delete(field);
    if (parsePullRequest(repo.value)) resume.set("pr", repo.value.trim());
    else {
      for (const field of ["repo", "ref", "base"]) {
        if (form.elements[field].value.trim()) resume.set(field, form.elements[field].value.trim());
      }
    }
  }
  resume.set("picker", "1");
  location.assign("/github/login?" + new URLSearchParams({ return: "/?" + resume }));
});
logout.addEventListener("click", async () => {
  const response = await fetch("/github/logout", { method: "POST" }).catch(() => null);
  if (response?.ok) location.replace("/");
});
group.addEventListener("change", renderLibrary);
choices.addEventListener("change", () => {
  if (!choices.value) return;
  repo.value = choices.value;
  for (const field of ["ref", "base"]) form.elements[field].value = "";
  repo.dispatchEvent(new Event("input"));
});

function showRevisions(items) {
  refs.replaceChildren();
  revisionOptions.replaceChildren(new Option("Choose a branch, tag, or commit", ""));
  for (const item of items) {
    refs.append(new Option(item.label, item.ref));
    revisionOptions.add(new Option(item.label, item.ref));
  }
  revisionOptions.disabled = !items.length;
  revisionHint.textContent = items.length ? "" : "No revision suggestions available.";
}

revisionOptions.addEventListener("change", () => {
  const key = document.querySelector("#revision-target").value;
  if (revisionOptions.value) form.elements[key].value = revisionOptions.value;
  revisionOptions.value = "";
});

export function suggestRevisions() {
  let name;
  try { name = parseRepository(repo.value); } catch { /* Still typing or using a PR. */ }
  document.querySelector("#revision-picker").hidden = !name;
  if (name === latest) return;
  clearTimeout(timer);
  latest = name;
  showRevisions([]);
  revisionHint.textContent = "";
  if (!name) return;
  timer = setTimeout(async () => {
    if (revisionCache.has(name)) {
      showRevisions(revisionCache.get(name) ?? []);
      return;
    }
    revisionHint.textContent = "Loading branches, tags, and commits…";
    try {
      const items = await loadRevisions(name, githubRequest);
      if (revisionCache.size >= 20) revisionCache.delete(revisionCache.keys().next().value);
      revisionCache.set(name, items);
      if (name !== latest) return;
      showRevisions(items);
    } catch {
      if (name === latest) revisionHint.textContent = "GitHub suggestions are unavailable.";
    }
  }, 360);
}
