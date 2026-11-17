const API = "https://api.github.com";
const MAX_BLOB_SIZE = 1024 * 1024;

export function parseRepository(input) {
  const match = /^(?:https:\/\/github\.com\/)?([\w.-]+)\/([\w.-]+?)(?:\.git)?\/?$/i.exec(input.trim());
  if (!match || match.slice(1).some(part => part === "." || part === "..")) {
    throw new Error("Enter owner/repository or a GitHub repository or pull request URL.");
  }
  return match.slice(1).join("/");
}

export function parsePullRequest(input) {
  if (typeof input !== "string") return null;
  const match = /^https:\/\/github\.com\/([\w.-]+)\/([\w.-]+)\/pull\/([1-9]\d{0,14})(?:\/(?:files|commits|checks))?\/?(?:[?#].*)?$/i.exec(input.trim());
  if (!match || [match[1], match[2]].some(part => part === "." || part === "..")) return null;
  return { name: `${match[1]}/${match[2]}`, number: match[3] };
}

const headers = { Accept: "application/vnd.github+json" };

async function readGitHub(request, path) {
  const response = await request(API + path, { headers });
  if (!response.ok) {
    if (response.status === 403 || response.status === 429) throw new Error("GitHub request limit or access restriction. Try again later.");
    if (response.status === 404) throw new Error("Repository, revision, or file not found. Private repositories need GitHub access.");
    throw new Error(`GitHub returned HTTP ${response.status}.`);
  }
  return response;
}

function commitSuggestion({ sha, commit }) {
  return { ref: sha, label: `${sha.slice(0, 7)} · ${commit.message.split("\n")[0]}` };
}

export async function searchCommits(input, query, request = fetch) {
  const repository = parseRepository(input);
  const words = (query.slice(0, 80).match(/[\p{L}\p{N}_-]+/gu) ?? []).slice(0, 8);
  if (words.join(" ").length < 3) return [];
  const q = `repo:${repository} ${words.map(word => `"${word}"`).join(" ")}`;
  const response = await readGitHub(request, "/search/commits?" + new URLSearchParams({ q, per_page: "8" }));
  const { items } = await response.json();
  return items.filter(item => item.repository?.full_name?.toLowerCase() === repository.toLowerCase())
    .map(commitSuggestion);
}

export async function loadRevisions(input, request = fetch) {
  const path = "/repos/" + parseRepository(input);
  const groups = await Promise.allSettled(["branches?per_page=100", "tags?per_page=50", "commits?per_page=30"]
    .map(async resource => (await readGitHub(request, `${path}/${resource}`)).json()));
  if (groups.every(group => group.status === "rejected")) throw groups[0].reason;
  const [branches, tags, commits] = groups.map(group =>
    group.status === "fulfilled" && Array.isArray(group.value) ? group.value : []);
  return [
    ...branches.map(({ name }) => ({ ref: `refs/heads/${name}`, label: `Branch · ${name}` })),
    ...tags.map(({ name }) => ({ ref: `refs/tags/${name}`, label: `Tag · ${name}` })),
    ...commits.map(commitSuggestion),
  ];
}

function pullState(pull) {
  return pull.merged || pull.merged_at ? "merged" : pull.state;
}

export async function loadPullRequests(input, request = fetch) {
  const repository = parseRepository(input);
  const entries = [];
  for (let page = 1; page <= 4; page++) {
    const response = await readGitHub(request, `/repos/${repository}/pulls?state=all&per_page=50&page=${page}`);
    const batch = await response.json();
    if (!Array.isArray(batch)) throw new Error("Pull requests are unavailable.");
    entries.push(...batch.filter(pull => Number.isSafeInteger(pull.number) && ["open", "closed"].includes(pull.state)));
    if (!response.headers.get("Link")?.includes('rel="next"')) break;
  }
  return entries.map(pull => ({
    value: `https://github.com/${repository}/pull/${pull.number}`,
    label: `#${pull.number} · ${{ open: "Open", closed: "Closed", merged: "Merged" }[pullState(pull)]} · ${pull.title} · @${pull.user?.login ?? "unknown"} · ${pull.head?.ref ?? "?"} → ${pull.base?.ref ?? "?"}`,
  }));
}

export async function loadLibrary(login, request) {
  async function pages(path, field) {
    const items = [];
    for (let page = 1; page <= 10; page++) {
      const separator = path.includes("?") ? "&" : "?";
      const response = await readGitHub(request, `${path}${separator}per_page=100&page=${page}`);
      const data = await response.json();
      items.push(...(field ? data[field] : data));
      if (!response.headers.get("link")?.includes('rel="next"')) break;
    }
    return items;
  }
  const [mine, stars, installations] = await Promise.allSettled([
    pages(`/users/${encodeURIComponent(login)}/repos?sort=pushed`),
    pages("/user/starred"),
    pages("/user/installations", "installations"),
  ]);
  const installed = await Promise.allSettled((installations.value ?? []).map(({ id }) =>
    pages(`/user/installations/${id}/repositories`, "repositories")));
  const grants = installed.flatMap(result => result.value ?? []);
  const own = [...(mine.value ?? []), ...grants];
  const byName = list => [...new Map(list.map(repo => [repo.full_name, repo])).values()]
    .sort((a, b) => a.full_name.localeCompare(b.full_name));
  return {
    repositories: byName(own.filter(repo => !repo.fork)),
    forks: byName(own.filter(repo => repo.fork)),
    stars: byName(stars.value ?? []),
    partial: [...[mine, stars, installations], ...installed].some(group => group.status === "rejected"),
  };
}

export async function loadGitHub(input, ref, base, request = fetch) {
  const pull = parsePullRequest(input);
  const name = pull?.name ?? parseRepository(input);
  const read = async (path, repository = name) =>
    (await readGitHub(request, `/repos/${repository}${path}`)).json();

  let head, baseline, status = null, headRepository = name;
  if (pull) {
    const details = await read(`/pulls/${pull.number}`);
    status = pullState(details);
    head = details.head;
    if (!head?.sha || !details.base?.sha) throw new Error("Pull request revisions unavailable.");
    if (head.repo?.full_name) headRepository = parseRepository(head.repo.full_name);
    baseline = (await read(`/compare/${encodeURIComponent(details.base.sha)}...${encodeURIComponent(head.sha)}`)).merge_base_commit;
    if (!baseline?.sha) throw new Error("Pull request merge base unavailable.");
  } else {
    const revision = ref?.trim() || (await read("")).default_branch;
    if (!revision) throw new Error("This repository has no default branch.");
    head = await read(`/commits/${encodeURIComponent(revision)}`);
    baseline = base?.trim()
      ? (await read(`/compare/${encodeURIComponent(base.trim())}...${head.sha}`)).merge_base_commit
      : head.parents[0];
  }

  const subtrees = new Map();
  function subtree(repository, sha) {
    const key = `${repository}:${sha}`;
    if (!subtrees.has(key)) subtrees.set(key, read(`/git/trees/${sha}`, repository));
    return subtrees.get(key);
  }

  async function inventory(commit, repository) {
    if (!commit) return {};
    const tree = await read(`/git/trees/${commit.sha}?recursive=1`, repository);
    const blob = entry => ({ id: entry.sha, mode: entry.mode, size: entry.size });
    if (!tree.truncated) return Object.fromEntries(tree.tree
      .filter(entry => entry.type === "blob").map(entry => [entry.path, blob(entry)]));
    const files = Object.create(null);
    const folders = [{ path: "", sha: tree.sha || commit.sha }];
    for (let offset = 0; offset < folders.length;) {
      const batch = folders.slice(offset, offset + 8);
      const contents = await Promise.all(batch.map(folder => subtree(repository, folder.sha)));
      contents.forEach(({ tree, truncated }, index) => {
        const prefix = batch[index].path;
        if (truncated) throw new Error(`GitHub truncated the directory ${prefix || "/"}.`);
        for (const entry of tree) {
          const path = prefix + entry.path;
          if (entry.type === "blob") files[path] = blob(entry);
          if (entry.type === "tree") folders.push({ path: path + "/", sha: entry.sha });
        }
      });
      offset += batch.length;
    }
    return files;
  }

  const [before, after] = await Promise.all([inventory(baseline, name), inventory(head, headRepository)]);
  if (!Object.keys(before).length && !Object.keys(after).length) throw new Error("This comparison has no files.");

  const forkBlobs = headRepository === name ? null : new Set(Object.values(after).map(file => file.id));
  function blameTarget(path, useBase) {
    if (!Object.hasOwn(useBase ? before : after, path)) return null;
    return {
      repository: useBase ? name : headRepository,
      sha: useBase ? baseline.sha : head.sha,
    };
  }
  return {
    review: { repository: name, pull: pull?.number ?? null, pullState: status, revision: head.sha, baseline: baseline?.sha ?? null },
    progressScope: JSON.stringify([name.toLowerCase(), head.sha, baseline?.sha ?? null]),
    snapshot: JSON.stringify({ name: pull ? `${name}#${pull.number}` : name, revision: head.sha, baseline: baseline?.sha ?? null, before, after }),
    blameURL(path, useBase, line) {
      const target = blameTarget(path, useBase);
      if (!target || !Number.isSafeInteger(line) || line < 1) return null;
      const file = path.split("/").map(encodeURIComponent).join("/");
      return `https://github.com/${target.repository}/blame/${encodeURIComponent(target.sha)}/${file}#L${line}`;
    },
    async blame(path, useBase, lineCount) {
      const target = blameTarget(path, useBase);
      if (!target) return { error: "unavailable" };
      const [owner, repo] = target.repository.split("/");
      const parameters = new URLSearchParams({ owner, repo, sha: target.sha, path });
      const response = await fetch("/github/blame?" + parameters, { signal: AbortSignal.timeout(15_000) });
      if (response.status === 401) return { error: "login" };
      if (!response.ok) return { error: "unavailable" };
      const { ranges } = await response.json();
      const notes = Array(lineCount).fill("");
      for (const { startingLine, endingLine, commit } of ranges) {
        if (!Number.isSafeInteger(startingLine) || !Number.isSafeInteger(endingLine) ||
            startingLine < 1 || endingLine < startingLine || startingLine > lineCount ||
            !commit?.abbreviatedOid || !commit.committedDate) continue;
        const author = commit.author?.name || commit.author?.user?.login || "Unknown author";
        const date = commit.committedDate.slice(0, 16).replace("T", " ");
        notes.fill(`${commit.abbreviatedOid.slice(0, 8)}  ${date}  ${author}`,
          startingLine - 1, Math.min(endingLine, lineCount));
      }
      return notes.some(Boolean) ? { notes } : { error: "unavailable" };
    },
    async blob(sha) {
      const blob = await read(`/git/blobs/${sha}`, forkBlobs?.has(sha) ? headRepository : name);
      if (blob.encoding !== "base64" || blob.size > MAX_BLOB_SIZE) throw new Error("Source unavailable within the 1 MiB text limit.");
      return Uint8Array.from(atob(blob.content), char => char.charCodeAt(0));
    },
  };
}
