import { loadLibrary, loadPullRequests, loadRevisions, parsePullRequest, parseRepository, searchCommits } from "./github.mjs";

const publicAPI = "https://api.github.com";
const account = document.querySelector("#github-account");
const loginButton = document.querySelector("#github-login");
const logout = document.querySelector("#github-logout");
const install = document.querySelector("#github-install");
const form = document.querySelector("#repository-form");
const { repo, ref, base } = form.elements;
const status = document.querySelector("#status");
const searchRow = document.querySelector("#repository-search");
const selection = document.querySelector("#repository-selection");
const change = document.querySelector("#change-repository");
const editor = document.querySelector("#revision-editor");
const revisionSearch = document.querySelector("#revision-search");
const revisionButton = document.querySelector("#choose-revision");
const baseButton = document.querySelector("#choose-base");
const pullButton = document.querySelector("#choose-pull");
const pullEditor = document.querySelector("#pull-editor");
const pullSearch = document.querySelector("#pull-search");
let revisionField;
let session, libraryData, libraryError, repository;
const revisionCache = new Map();
const searchCache = new Map();
const pullCache = new Map();

export async function githubRequest(url, options) {
  await ready;
  if (session?.login && url.startsWith(publicAPI + "/")) {
    const path = url.slice(publicAPI.length);
    const response = await fetch("/github/api" + path, options);
    if (response.status === 401 && session?.login) {
      session = { ...session, login: null };
      libraryData = null;
      revisionCache.clear();
      searchCache.clear();
      pullCache.clear();
      paintAccount();
      revisionChoices.refresh();
      repoChoices.refresh();
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

export const githubAccount = () => session;
export const githubReady = () => ready;
export const githubSignIn = () => "/github/login?return=" + encodeURIComponent(location.pathname + location.search);

function paintAccount() {
  const login = session?.login;
  loginButton.hidden = !session?.enabled || Boolean(login);
  account.hidden = !login;
  repo.placeholder = login ? "Find a repository or paste a PR URL" : "Enter owner/repository or a PR URL";
  install.hidden = !session?.installUrl;
  if (session?.installUrl) install.href = session.installUrl;
  const user = document.querySelector("#github-user");
  user.textContent = login ? "@" + login : "";
  user.title = user.textContent;
}

async function fillLibrary() {
  const login = session.login;
  repoChoices.refresh();
  try {
    const result = await loadLibrary(login, githubRequest);
    if (session?.login === login) libraryData = result;
  } catch {
    libraryError = true;
  }
  repoChoices.refresh();
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

form.addEventListener("focusin", event => {
  if (!account.contains(event.target)) account.open = false;
});
logout.addEventListener("click", async () => {
  const response = await fetch("/github/logout", { method: "POST" }).catch(() => null);
  if (response?.ok) location.replace("/");
});

function suggest(input, container, query, onPick) {
  const panel = document.createElement("div");
  panel.className = "suggestions";
  panel.hidden = true;
  const list = document.createElement("ul");
  list.id = input.id + "-options";
  list.setAttribute("role", "listbox");
  const label = input.labels[0];
  label.id = input.id + "-label";
  list.setAttribute("aria-labelledby", label.id);
  const status = document.createElement("small");
  status.setAttribute("role", "status");
  panel.append(list, status);
  container.append(panel);
  input.setAttribute("role", "combobox");
  input.setAttribute("aria-autocomplete", "list");
  input.setAttribute("aria-controls", list.id);
  input.setAttribute("aria-expanded", "false");
  let revision = 0, active = -1, options = [];

  function highlight(index) {
    active = index;
    for (const [i, row] of Array.from(list.children).entries()) row.setAttribute("aria-selected", i === active);
    input.removeAttribute("aria-activedescendant");
    if (active < 0) return;
    const row = list.children[active];
    input.setAttribute("aria-activedescendant", row.id);
    row.scrollIntoView({ block: "nearest" });
  }

  function close() {
    revision++;
    panel.hidden = true;
    input.setAttribute("aria-expanded", "false");
    input.removeAttribute("aria-activedescendant");
    active = -1;
    options = [];
  }

  function select(index) {
    const item = options[index];
    if (!item) return;
    input.value = item.value;
    close();
    onPick?.();
  }

  function refresh() {
    if (document.activeElement !== input || input.disabled) return close();
    const current = ++revision;
    function show(matches, message = "") {
      if (current !== revision) return;
      const selected = options[active]?.value;
      options = matches.slice(0, 8);
      list.replaceChildren(...options.map(({ label }, i) => {
        const row = document.createElement("li");
        row.id = list.id + "-" + i;
        row.textContent = label;
        row.setAttribute("role", "option");
        row.setAttribute("aria-selected", "false");
        return row;
      }));
      status.textContent = message;
      status.hidden = !message;
      if (!options.length && !message) {
        close();
        return;
      }
      panel.hidden = false;
      input.setAttribute("aria-expanded", "true");
      highlight(options.findIndex(item => item.value === selected));
    }
    query(input.value, show, () => revision === current);
  }

  input.addEventListener("focus", refresh);
  input.addEventListener("input", refresh);
  input.addEventListener("blur", close);
  input.addEventListener("keydown", event => {
    if (event.isComposing || panel.hidden) return;
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp":
        if (!options.length) return;
        highlight(active < 0
          ? (event.key === "ArrowDown" ? 0 : options.length - 1)
          : (active + (event.key === "ArrowDown" ? 1 : -1) + options.length) % options.length);
        break;
      case "Enter":
        if (active < 0) return;
        select(active);
        break;
      case "Escape":
        close();
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  });
  list.addEventListener("pointerdown", event => event.preventDefault());
  list.addEventListener("click", event => {
    const row = event.target.closest('[role="option"]');
    if (row) select(Array.from(list.children).indexOf(row));
  });
  return { refresh, close };
}

function matches(text, query) {
  const folded = text.toLowerCase();
  return query.toLowerCase().trim().split(/\s+/).every(term => folded.includes(term));
}

const repoChoices = suggest(repo, document.querySelector("#repository-results"), (query, show) => {
  if (!session?.login || /^https?:/i.test(query.trim())) return show([]);
  if (!libraryData) return show([], libraryError ? "GitHub repositories unavailable." : "Loading repositories…");
  const seen = new Set();
  const items = [];
  for (const [name, label] of [["repositories", ""], ["forks", "Fork"], ["stars", "Star"]]) {
    for (const entry of libraryData[name]) {
      const value = entry.full_name;
      if (!matches(value, query) || seen.has(value.toLowerCase())) continue;
      seen.add(value.toLowerCase());
      const qualifiers = [entry.private && "Private", label].filter(Boolean);
      items.push({ value, label: [value, ...qualifiers].join(" · ") });
      if (items.length === 8) break;
    }
    if (items.length === 8) break;
  }
  show(items, items.length ? "" : (libraryData.partial ? "Some GitHub repositories are unavailable." : "No matches in your repositories, stars, or forks."));
}, () => selectRepository(true));

function cached(cache, key, load) {
  if (cache.has(key)) return cache.get(key);
  if (cache.size >= 24) cache.delete(cache.keys().next().value);
  const result = load().catch(error => {
    if (cache.get(key) === result) cache.delete(key);
    throw error;
  });
  cache.set(key, result);
  return result;
}

async function revisions(query, show, current) {
  const name = repository;
  if (!name) return show([]);
  show([], "Loading revisions…");
  let items = [];
  const canSearch = query.trim().length >= 3 && !query.startsWith("refs/");
  try {
    const recent = await cached(revisionCache, name, () => loadRevisions(name, githubRequest));
    items = recent.filter(({ ref, label }) => matches(ref + " " + label, query));
    show(items.map(({ ref, label }) => ({ value: ref, label })), items.length || !canSearch ? "" : "Searching commits…");
  } catch {
    show([], canSearch ? "Searching commits…" : "GitHub suggestions unavailable.");
  }
  if (!canSearch || items.length >= 8) return;
  await new Promise(resolve => setTimeout(resolve, 360));
  if (!current()) return;
  try {
    const more = await cached(searchCache, JSON.stringify([name, query]), () => searchCommits(name, query, githubRequest));
    const unique = new Map(items.map(item => [item.ref, item]));
    for (const item of more) if (!unique.has(item.ref)) unique.set(item.ref, item);
    show(Array.from(unique.values(), ({ ref, label }) => ({ value: ref, label })), unique.size ? "" : "No matching revisions.");
  } catch {
    if (!items.length) show([], "GitHub commit search unavailable.");
  }
}

const revisionChoices = suggest(revisionSearch, document.querySelector("#revision-results"), revisions, saveRevision);

function updateRevisionButtons() {
  for (const [field, button, label, placeholder] of [
    [ref, revisionButton, "Revision", "Default branch"],
    [base, baseButton, "Compare with", "Compare…"],
  ]) {
    const value = field.value.replace(/^refs\/(heads|tags)\//, "");
    const short = /^[a-f0-9]{40}$/i.test(value) ? value.slice(0, 7) : value;
    button.textContent = short || placeholder;
    button.setAttribute("aria-label", label + ": " + (short || (field === base ? "Parent commit" : placeholder)));
    button.title = value || (field === base ? "Parent commit" : placeholder);
    button.setAttribute("aria-expanded", field === revisionField);
  }
}

function showRevision(field) {
  closePull();
  if (revisionField === field) return closeRevision(true);
  revisionField = field;
  editor.hidden = false;
  revisionSearch.value = "";
  revisionSearch.placeholder = field.value || "Find a branch, tag, or commit";
  updateRevisionButtons();
  revisionSearch.focus();
  revisionChoices.refresh();
}

function closeRevision(restoreFocus = false) {
  const field = revisionField;
  revisionField = null;
  revisionChoices.close();
  editor.hidden = true;
  updateRevisionButtons();
  if (restoreFocus && field) (field === ref ? revisionButton : baseButton).focus();
}

function saveRevision() {
  if (!revisionField) return;
  revisionField.value = revisionSearch.value.trim();
  closeRevision(true);
}

const pullChoices = suggest(pullSearch, document.querySelector("#pull-results"), async (query, show, current) => {
  if (!repository) return show([]);
  try {
    show([], "Loading pull requests…");
    const pulls = await cached(pullCache, repository, () => loadPullRequests(repository, githubRequest));
    if (!current()) return;
    const term = query.trim().toLowerCase();
    const matches = pulls.filter(pull => pull.label.toLowerCase().includes(term));
    show(matches, matches.length ? "" : "No matching pull requests.");
  } catch (error) {
    show([], error.message);
  }
}, () => {
  repo.value = pullSearch.value;
  ref.value = base.value = "";
  selectRepository(true);
});

function closePull() {
  pullEditor.hidden = true;
  pullButton.setAttribute("aria-expanded", "false");
  pullChoices.close();
}

pullButton.addEventListener("click", () => {
  if (!pullEditor.hidden) return closePull();
  closeRevision();
  pullSearch.value = "";
  pullEditor.hidden = false;
  pullButton.setAttribute("aria-expanded", "true");
  pullSearch.focus();
});
pullSearch.addEventListener("keydown", event => {
  if (event.defaultPrevented || event.isComposing) return;
  if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); closePull(); pullButton.focus(); }
  if (event.key === "Enter") event.preventDefault();
});

function selectRepository(focusNext = false) {
  const pull = parsePullRequest(repo.value);
  try {
    repository = pull ? null : parseRepository(repo.value);
  } catch {
    if (focusNext) {
      status.textContent = "Enter owner/repository or paste a GitHub pull request URL.";
      repo.focus();
    }
    return;
  }
  const name = pull ? pull.name + " #" + pull.number : repository;
  document.querySelector("#selected-repository").textContent = name;
  document.querySelector("#revisions").hidden = Boolean(pull);
  repoChoices.close();
  searchRow.hidden = true;
  selection.hidden = false;
  status.textContent = "";
  closeRevision();
  closePull();
  if (focusNext) document.querySelector("#open-city").focus();
}

export function refreshRepositoryPicker() {
  if (repo.value) selectRepository();
}

export function focusRepositoryPicker() {
  (selection.hidden ? repo : change).focus();
}

document.querySelector("#select-repository").addEventListener("click", () => selectRepository(true));
change.addEventListener("click", () => {
  repo.value = ref.value = base.value = "";
  repository = null;
  closeRevision();
  closePull();
  selection.hidden = true;
  searchRow.hidden = false;
  status.textContent = "";
  repo.focus();
});
revisionButton.addEventListener("click", () => showRevision(ref));
baseButton.addEventListener("click", () => showRevision(base));
document.querySelector("#use-revision").addEventListener("click", saveRevision);
repo.addEventListener("input", () => { status.textContent = ""; });
repo.addEventListener("keydown", event => {
  if (event.key === "Enter" && !event.isComposing && !event.defaultPrevented) {
    event.preventDefault();
    selectRepository(true);
  }
});
revisionSearch.addEventListener("keydown", event => {
  if (event.isComposing || event.defaultPrevented) return;
  if (event.key === "Enter") {
    event.preventDefault();
    saveRevision();
  } else if (event.key === "Escape") {
    event.preventDefault();
    closeRevision(true);
  }
});
