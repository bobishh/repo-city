// Keep publishing scoped to one review comment at GitHub's current diff coordinates.
const noteId = /^[a-f\d]{8}(?:-[a-f\d]{4}){3}-[a-f\d]{12}$/i;
const presetId = /^[a-z\d-]{1,80}$/i;
const sha = /^[a-f\d]{40}$/i;
const fail = (status, error, extra) => ({ status, value: { error, ...extra } });
const metadata = (id, preset) => `<!-- repo-city:v1:${id}:${preset} -->`;
export const replacementLimit = 8000;
export function suggestionBlock(replacement) {
  const fence = "`".repeat(Math.max(3, ...(replacement.match(/`+/g) ?? []).map(run => run.length + 1)));
  return `${fence}suggestion\n${replacement}${replacement.endsWith("\n") ? "" : "\n"}${fence}`;
}
export function commentText(text, id, preset, replacement) {
  return [text.trim(), replacement === undefined ? "" : suggestionBlock(replacement), metadata(id, preset)]
    .filter(Boolean).join("\n\n");
}
export const readCommentText = body => {
  const match = /\n\n<!-- repo-city:v1:([a-f\d-]{36}):([a-z\d-]{1,80}) -->$/i.exec(body ?? "");
  return match ? { text: body.slice(0, match.index), id: match[1], preset: match[2] } : { text: body ?? "" };
};

export function containsRange(patch, before, start, end) {
  if (typeof patch !== "string") return false;
  let line = 0, count = 0, match = 0;
  for (const row of patch.split("\n")) {
    const hunk = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(row);
    if (hunk) {
      line = Number(hunk[before ? 1 : 3]);
      count = Number(hunk[before ? 2 : 4] ?? "1");
      match = 0;
      continue;
    }
    if (!count || (row[0] !== " " && row[0] !== (before ? "-" : "+"))) continue;
    if (line === start + match) {
      match++;
      if (line === end) return true;
    }
    line++;
    count--;
  }
  return false;
}

const validPath = value => typeof value === "string" && value.length <= 4096 &&
  !/[\u0000-\u001f]/u.test(value) && value.split("/").every(part => part && part !== "." && part !== "..");

function validIdentity({ id, revision, preset }) {
  // The local note ID makes retries idempotent; the preset survives in comment metadata.
  // The revision identifies the GitHub PR head that the reviewer actually inspected.
  return typeof id === "string" && noteId.test(id) &&
    typeof revision === "string" && sha.test(revision) &&
    typeof preset === "string" && presetId.test(preset);
}

function validAnchor({ path, before, start, end, fileLevel }) {
  // GitHub uses inclusive positive line numbers and separate LEFT/RIGHT diff sides.
  // Membership in the current PR and a single hunk is checked against GitHub below.
  return validPath(path) && typeof before === "boolean" &&
    Number.isSafeInteger(start) && start >= 1 && Number.isSafeInteger(end) && end >= start &&
    (fileLevel === undefined || typeof fileLevel === "boolean");
}

function validBody({ text }, suggestion) {
  // A replacement suggestion can supply the whole body without explanatory text.
  return typeof text === "string" && text.length <= 6000 && (suggestion || Boolean(text.trim()));
}

export async function readSmallJson(request) {
  if (!request.headers.get("Content-Type")?.toLowerCase().startsWith("application/json") || !request.body) return null;
  const reader = request.body.getReader();
  const decode = new TextDecoder("utf-8", { fatal: true });
  try {
    let length = 0, text = "";
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return JSON.parse(text + decode.decode());
      if ((length += value.byteLength) > 96 * 1024) { await reader.cancel(); return null; }
      text += decode.decode(value, { stream: true });
    }
  } catch {
    await reader.cancel().catch(() => {});
    return null;
  } finally {
    reader.releaseLock();
  }
}

export async function writeReviewComment(input, login, call) {
  const draft = input ?? {};
  const { id, revision, path, before, start, end, preset, text, fileLevel, replacement } = draft;
  const suggestion = replacement !== undefined;
  if (!validIdentity(draft) || !validAnchor(draft) || !validBody(draft, suggestion)) {
    return fail(400, "Invalid draft comment.");
  }
  if (suggestion && (fileLevel || before || typeof replacement !== "string" || !replacement.length ||
      replacement.length > replacementLimit)) {
    return fail(400, "A suggestion needs replacement code and lines on the current side of the diff.");
  }
  // A previous response may have been lost after GitHub stored the comment.
  for (let page = 1; page <= 12; page++) {
    const response = await call(`/comments?sort=created&direction=desc&per_page=100&page=${page}`);
    if (!response.ok) return accessError(response);
    const comments = await response.json();
    const existing = comments.find(item => item.user?.login === login && readCommentText(item.body).id === id);
    if (existing) return { status: 200, value: { comment: existing } };
    if (!response.headers.get("Link")?.includes('rel="next"')) break;
    if (page === 12) return fail(409, "Too many comments to check safely. Check GitHub before retrying.");
  }
  const pull = await call("");
  if (!pull.ok) return accessError(pull);
  const snapshot = await pull.json();
  if (snapshot.head?.sha !== revision) return fail(409, "The pull request changed. Open the current revision and reselect the lines.");
  let file;
  for (let page = 1; page <= 30; page++) {
    const response = await call(`/files?per_page=100&page=${page}`);
    if (!response.ok) return accessError(response);
    file = (await response.json()).find(item => item.filename === path || (before && item.previous_filename === path));
    if (file || !response.headers.get("Link")?.includes('rel="next"')) break;
  }
  if (!file) return fail(422, "That file is not in the current GitHub diff. Keep your draft or reselect a changed file.");
  if (!fileLevel && !containsRange(file.patch, before, start, end)) {
    if (suggestion) return fail(422, "Select lines within one GitHub diff hunk. Your suggestion is still saved.");
    return fail(422, "That range is outside a single GitHub diff hunk. Keep it private, reselect the lines, or publish at file level.", { fileLevel: true });
  }
  const side = before ? "LEFT" : "RIGHT";
  const body = commentText(text, id, preset, replacement);
  let response;
  try {
    response = await call("/comments", {
      body, commit_id: revision, path: file.filename,
      ...(fileLevel ? { subject_type: "file" } : { line: end, side, ...(start === end ? {} : { start_line: start, start_side: side }) }),
    });
  } catch {
    return fail(502, "GitHub did not confirm the write. Retry to check for the existing comment.");
  }
  if (response.status === 422) {
    await response.body?.cancel();
    return fail(422, "GitHub rejected the anchor. Your draft is still saved.", { fileLevel: !fileLevel && !suggestion });
  }
  if (!response.ok) return accessError(response);
  return { status: 201, value: { comment: await response.json() } };
}

async function accessError(response) {
  await response.body?.cancel();
  if (response.status === 401) return fail(401, "Sign in with GitHub again to publish.");
  if ([403, 404].includes(response.status)) return fail(403, "Publishing needs access to this repository and Pull requests: write on the Repo City GitHub App.");
  if (response.status === 429) return fail(429, "GitHub rate limit reached. Your draft is still saved.");
  return fail(502, "GitHub did not confirm the request. Your draft is still saved.");
}
