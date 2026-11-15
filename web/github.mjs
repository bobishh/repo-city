const API = "https://api.github.com";
const MAX_FILES = 5000;
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

export async function loadGitHub(input, ref, base, request = fetch) {
  const pull = parsePullRequest(input);
  const name = pull?.name ?? parseRepository(input);
  async function read(path, repository = name) {
    const response = await request(`${API}/repos/${repository}${path}`, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) {
      if (response.status === 403 || response.status === 429) throw new Error("GitHub request limit or access restriction. Try again later.");
      if (response.status === 404) throw new Error("Public repository, revision, or file not found.");
      throw new Error(`GitHub returned HTTP ${response.status}.`);
    }
    return response.json();
  }

  let head, baseline, headRepository = name;
  if (pull) {
    const details = await read(`/pulls/${pull.number}`);
    if (details.state !== "open") throw new Error("Repo City currently opens only open pull requests.");
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

  async function inventory(commit, repository) {
    if (!commit) return {};
    const tree = await read(`/git/trees/${commit.sha}?recursive=1`, repository);
    if (tree.truncated) throw new Error("GitHub truncated this tree. Choose a smaller repository.");
    return Object.fromEntries(tree.tree.filter(entry => entry.type === "blob").map(entry => [
      entry.path, { id: entry.sha, mode: entry.mode, size: entry.size },
    ]));
  }

  const [before, after] = await Promise.all([inventory(baseline, name), inventory(head, headRepository)]);
  const paths = new Set([...Object.keys(before), ...Object.keys(after)]);
  if (!paths.size || paths.size > MAX_FILES) throw new Error(`Repo City supports 1 to ${MAX_FILES} files per comparison.`);

  const forkBlobs = headRepository === name ? null : new Set(Object.values(after).map(file => file.id));
  return {
    snapshot: JSON.stringify({ name: pull ? `${name}#${pull.number}` : name, revision: head.sha, baseline: baseline?.sha ?? null, before, after }),
    blameURL(path, useBase, line) {
      if (!Number.isSafeInteger(line) || line < 1 || !Object.hasOwn(useBase ? before : after, path)) return null;
      const repository = useBase ? name : headRepository;
      const commit = useBase ? baseline.sha : head.sha;
      const file = path.split("/").map(encodeURIComponent).join("/");
      return `https://github.com/${repository}/blame/${encodeURIComponent(commit)}/${file}#L${line}`;
    },
    async blob(sha) {
      const blob = await read(`/git/blobs/${sha}`, forkBlobs?.has(sha) ? headRepository : name);
      if (blob.encoding !== "base64" || blob.size > MAX_BLOB_SIZE) throw new Error("Source unavailable within the 1 MiB text limit.");
      return Uint8Array.from(atob(blob.content), char => char.charCodeAt(0));
    },
  };
}
