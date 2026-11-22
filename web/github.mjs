const API = "https://api.github.com";

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

async function readGitHub(request, path, publicObject = false) {
  const response = await request(API + path, publicObject ? { headers, publicObject } : { headers });
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
  async function list(state) {
    const pulls = [];
    for (let page = 1; page <= 4; page++) {
      const response = await readGitHub(request, `/repos/${repository}/pulls?state=${state}&per_page=50&page=${page}`);
      const batch = await response.json();
      if (!Array.isArray(batch)) throw new Error("Pull requests are unavailable.");
      pulls.push(...batch.filter(pull => Number.isSafeInteger(pull.number) && pull.state === state));
      if (!response.headers.get("Link")?.includes('rel="next"')) break;
    }
    return pulls;
  }
  const groups = await Promise.allSettled([list("open"), list("closed")]);
  if (groups.every(group => group.status === "rejected")) throw groups[0].reason;
  const entries = groups.flatMap(group => group.status === "fulfilled" ? group.value : []);
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

export async function loadGitHub(input, ref, base, request = fetch, { exactBase = false, pinnedComparison } = {}) {
  const pull = parsePullRequest(input);
  const name = pull?.name ?? parseRepository(input);
  const publicRepositories = new Set();
  const noteVisibility = (repository, info) => {
    if (info?.private === false && (info.visibility == null || info.visibility === "public")) {
      publicRepositories.add(repository.toLowerCase());
    }
  };
  const read = async (path, repository = name, publicObject = false) =>
    (await readGitHub(request, `/repos/${repository}${path}`, publicObject)).json();
  const readObject = (path, repository) => read(path, repository, publicRepositories.has(repository.toLowerCase()));

  let head, baseline, comparison, comparePath, status = null, headRepository = name;
  async function compare(base, revision) {
    comparePath = `/compare/${encodeURIComponent(base)}...${encodeURIComponent(revision)}`;
    comparison = await read(comparePath);
    return comparison.merge_base_commit;
  }
  if (pull) {
    const details = await read(`/pulls/${pull.number}`);
    status = pullState(details);
    head = details.head;
    if (!head?.sha || !details.base?.sha) throw new Error("Pull request revisions unavailable.");
    if (head.repo?.full_name) headRepository = parseRepository(head.repo.full_name);
    noteVisibility(name, details.base.repo);
    noteVisibility(headRepository, head.repo);
    if (pinnedComparison) {
      headRepository = pinnedComparison.head_owner && pinnedComparison.head_repo
        ? parseRepository(`${pinnedComparison.head_owner}/${pinnedComparison.head_repo}`) : headRepository;
      head = await read(`/commits/${encodeURIComponent(pinnedComparison.head_sha)}`, headRepository);
      baseline = pinnedComparison.base_sha ? await read(`/commits/${encodeURIComponent(pinnedComparison.base_sha)}`) : null;
    } else baseline = await compare(details.base.sha, head.sha);
    if (!baseline?.sha) throw new Error("Pull request merge base unavailable.");
  } else {
    let revision = ref?.trim();
    if (!revision) {
      const info = await read("");
      noteVisibility(name, info);
      revision = info.default_branch;
    }
    if (!revision) throw new Error("This repository has no default branch.");
    head = await read(`/commits/${encodeURIComponent(revision)}`);
    baseline = base?.trim()
      ? (exactBase ? await read(`/commits/${encodeURIComponent(base.trim())}`) : await compare(base.trim(), head.sha))
      : head.parents[0];
  }

  const subtrees = new Map();
  function subtree(repository, sha) {
    const key = `${repository}:${sha}`;
    if (!subtrees.has(key)) subtrees.set(key, readObject(`/git/trees/${sha}`, repository));
    return subtrees.get(key);
  }

  async function inventory(commit, repository) {
    if (!commit) return {};
    const tree = await readObject(`/git/trees/${commit.sha}?recursive=1`, repository);
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
  let fileDates;
  function loadChangeDates(paths) {
    if (!fileDates) fileDates = (async () => {
      const remaining = new Set(paths);
      const dates = Object.create(null);
      if (!remaining.size) return dates;
      let commits = comparison?.commits ?? (comparePath ? [] : [head]);
      if (comparePath && comparison?.total_commits > commits.length) {
        commits = [];
        for (let page = 1; commits.length < comparison.total_commits; page++) {
          const batch = (await read(`${comparePath}?per_page=100&page=${page}`)).commits;
          if (!batch?.length) break;
          commits.push(...batch);
        }
      }
      const date = info => Date.parse(info.commit?.committer?.date);
      commits = commits.filter(info => Number.isFinite(date(info))).sort((a, b) => date(b) - date(a));
      for (let offset = 0; remaining.size && offset < commits.length; offset += 4) {
        const batch = await Promise.allSettled(commits.slice(offset, offset + 4).map(async info => {
          if (Array.isArray(info.files) && info.files.length < 300) return info;
          const files = [];
          for (let page = 1;; page++) {
            const query = `/commits/${info.sha}?per_page=100&page=${page}`;
            let details;
            try { details = await read(query, headRepository); }
            catch (error) {
              if (headRepository === name) throw error;
              details = await read(query);
            }
            const entries = details.files ?? [];
            files.push(...entries);
            if (entries.length < 100) break;
          }
          return { ...info, files };
        }));
        let partial = false;
        for (const item of batch) {
          if (item.status === "rejected") {
            partial = true;
            continue;
          }
          const { files } = item.value;
          const time = date(item.value);
          for (const { filename, previous_filename } of files) {
            for (const path of [filename, previous_filename]) {
              if (remaining.delete(path)) dates[path] = time;
            }
          }
        }
        if (partial) break;
      }
      return dates;
    })();
    return fileDates;
  }

  return {
    review: { repository: name, pull: pull?.number ?? null, pullState: status, revision: head.sha, baseline: baseline?.sha ?? null },
    progressScope: JSON.stringify([name.toLowerCase(), head.sha, baseline?.sha ?? null]),
    file(path, useBase) {
      const files = useBase ? before : after;
      return Object.hasOwn(files, path) ? files[path] : undefined;
    },
    loadChangeDates,
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
      const blob = await readObject(`/git/blobs/${sha}`, forkBlobs?.has(sha) ? headRepository : name);
      if (blob.encoding !== "base64" || typeof blob.content !== "string") {
        throw new Error("GitHub did not return the file contents.");
      }
      if (Uint8Array.fromBase64) return Uint8Array.fromBase64(blob.content);
      const decoded = atob(blob.content);
      const bytes = new Uint8Array(decoded.length);
      for (let index = 0; index < decoded.length; index++) bytes[index] = decoded.charCodeAt(index);
      return bytes;
    },
  };
}

export async function loadCommitHistory(input, revision, page = 1, request = fetch) {
  const repository = parseRepository(input);
  if (!Number.isSafeInteger(page) || page < 1) throw new Error("Invalid history page.");
  const query = new URLSearchParams({ sha: revision, per_page: "100", page: String(page) });
  const rows = await (await readGitHub(request, `/repos/${repository}/commits?${query}`)).json();
  if (!Array.isArray(rows)) throw new Error("GitHub commit history is unavailable.");
  return rows.filter(row => /^[a-f\d]{40,64}$/i.test(row.sha)).map(row => ({
    id: row.sha, summary: String(row.commit?.message ?? "").split("\n")[0],
    author: row.commit?.author?.name ?? "Unknown", date: row.commit?.author?.date ?? "",
  }));
}

export async function loadCommitDetails(input, id, request = fetch) {
  const repository = parseRepository(input);
  if (!/^(?:[a-f\d]{40}|[a-f\d]{64})$/i.test(id)) throw new Error("Invalid commit identifier.");
  let text = "";
  for (let page = 1;; page++) {
    const response = await readGitHub(request, `/repos/${repository}/commits/${id}?per_page=100&page=${page}`);
    const commit = await response.json();
    if (page === 1) text = `${commit.sha}\n${commit.commit?.author?.name ?? ""} · ${commit.commit?.author?.date ?? ""}\n\n${commit.commit?.message ?? ""}\n`;
    for (const file of commit.files ?? []) text += `\n${file.filename}\n${file.patch ?? "Patch unavailable (binary or omitted by GitHub)."}\n`;
    if (text.length > 2 * 1024 * 1024) return text.slice(0, 2 * 1024 * 1024) + "\n[Commit preview truncated.]";
    if (!response.headers.get("Link")?.includes('rel="next"')) return text;
  }
}
