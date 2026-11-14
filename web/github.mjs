const API = "https://api.github.com";
const MAX_FILES = 5000;
const MAX_BLOB_SIZE = 1024 * 1024;

export function parseRepository(input) {
  const match = /^(?:https:\/\/github\.com\/)?([\w.-]+)\/([\w.-]+?)(?:\.git)?\/?$/i.exec(input.trim());
  if (!match || match.slice(1).some(part => part === "." || part === "..")) {
    throw new Error("Enter owner/repository or a GitHub repository URL.");
  }
  return match.slice(1).join("/");
}

export async function loadGitHub(input, ref, base, request = fetch) {
  const name = parseRepository(input);
  async function read(path) {
    const response = await request(`${API}/repos/${name}${path}`, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) {
      if (response.status === 403 || response.status === 429) throw new Error("GitHub request limit or access restriction. Try again later.");
      if (response.status === 404) throw new Error("Public repository, revision, or file not found.");
      throw new Error(`GitHub returned HTTP ${response.status}.`);
    }
    return response.json();
  }

  const resolve = revision => read(`/commits/${encodeURIComponent(revision)}`);
  const revision = ref?.trim() || (await read("")).default_branch;
  if (!revision) throw new Error("This repository has no default branch.");
  const head = await resolve(revision);
  const baseline = base?.trim()
    ? (await read(`/compare/${encodeURIComponent(base.trim())}...${head.sha}`)).merge_base_commit
    : head.parents[0];

  async function inventory(commit) {
    if (!commit) return {};
    const tree = await read(`/git/trees/${commit.sha}?recursive=1`);
    if (tree.truncated) throw new Error("GitHub truncated this tree. Choose a smaller repository.");
    return Object.fromEntries(tree.tree.filter(entry => entry.type === "blob").map(entry => [
      entry.path, { id: entry.sha, mode: entry.mode, size: entry.size },
    ]));
  }

  const [before, after] = await Promise.all([inventory(baseline), inventory(head)]);
  const paths = new Set([...Object.keys(before), ...Object.keys(after)]);
  if (!paths.size || paths.size > MAX_FILES) throw new Error(`Repo City supports 1 to ${MAX_FILES} files per comparison.`);

  return {
    snapshot: JSON.stringify({ name, revision: head.sha, baseline: baseline?.sha ?? null, before, after }),
    async blob(sha) {
      const blob = await read(`/git/blobs/${sha}`);
      if (blob.encoding !== "base64" || blob.size > MAX_BLOB_SIZE) throw new Error("Source unavailable within the 1 MiB text limit.");
      return Uint8Array.from(atob(blob.content), char => char.charCodeAt(0));
    },
  };
}
