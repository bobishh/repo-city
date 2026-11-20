const api = "https://api.github.com";
const prefix = "/github/public";
const maxObjectBytes = 4 * 1024 * 1024;
const maxCacheBytes = 24 * 1024 * 1024;
const maxEntries = 128;
const maxInFlight = 8;
const retentionMs = 15 * 60_000;
const objects = new Map();
const inFlight = new Map();
let cachedBytes = 0;
let pausedUntil = 0;

function targetFor(url) {
  const path = url.pathname.slice(prefix.length);
  const match = /^\/repos\/([\w.-]{1,100})\/([\w.-]{1,100})\/git\/(blobs|trees)\/([a-f\d]{40,64})$/i.exec(path);
  if (!match || match.slice(1, 3).some(name => name === "." || name === "..")) return null;
  if (url.search && !(match[3].toLowerCase() === "trees" && url.search === "?recursive=1")) return null;
  return new URL(path.toLowerCase() + url.search, api);
}

function remove(key) {
  const previous = objects.get(key);
  if (previous) cachedBytes -= previous.body.byteLength;
  objects.delete(key);
}

function store(key, body, etag) {
  remove(key);
  while (objects.size >= maxEntries || cachedBytes + body.byteLength > maxCacheBytes) {
    remove(objects.keys().next().value);
  }
  objects.set(key, { body, etag, expires: Date.now() + retentionMs });
  cachedBytes += body.byteLength;
}

function publicResponse(response) {
  const directives = (response.headers.get("Cache-Control") ?? "").split(",")
    .map(value => value.trim().split("=", 1)[0].toLowerCase());
  return directives.includes("public") && !directives.includes("private") &&
    !directives.includes("no-store") && !response.headers.has("Set-Cookie");
}

async function boundedBody(response) {
  if (!response.body || Number(response.headers.get("Content-Length")) > maxObjectBytes) {
    await response.body?.cancel();
    return null;
  }
  const reader = response.body.getReader();
  const chunks = [];
  let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > maxObjectBytes) {
        await reader.cancel();
        return null;
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }
  const body = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    body.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return body;
}

async function fetchPublic(target, upstream) {
  const key = target.href;
  if (objects.get(key)?.expires <= Date.now()) remove(key);
  const saved = objects.get(key);
  const headers = {
    Accept: "application/vnd.github+json",
    "User-Agent": "Repo City",
    "X-GitHub-Api-Version": "2022-11-28",
    "Cache-Control": "no-cache",
    ...(saved && { "If-None-Match": saved.etag }),
  };
  try {
    const response = await upstream(target, { headers, redirect: "manual", signal: AbortSignal.timeout(8000) });
    const etag = response.headers.get("ETag");
    if (response.status === 304 && saved && publicResponse(response) && (!etag || etag === saved.etag)) {
      store(key, saved.body, saved.etag);
      return { body: saved.body, cache: "REVALIDATED" };
    }
    remove(key);
    if (response.status === 429 || (response.status === 403 && response.headers.get("X-RateLimit-Remaining") === "0")) {
      const now = Date.now();
      const retryMs = Number(response.headers.get("Retry-After")) * 1000;
      const resetMs = Number(response.headers.get("X-RateLimit-Reset")) * 1000;
      pausedUntil = Math.max(pausedUntil, now + 60_000, now + (retryMs || 0), resetMs || 0);
    }
    if (response.status !== 200 || !publicResponse(response) ||
        !/^application\/(?:json|vnd\.github\+json)\b/i.test(response.headers.get("Content-Type") ?? "")) {
      await response.body?.cancel();
      const status = [403, 404, 429, 503].includes(response.status) ? response.status : 404;
      return { status };
    }
    const body = await boundedBody(response);
    if (!body) return { status: 413 };
    if (etag && /^(?:W\/)?"[\x21\x23-\x7e]{1,256}"$/.test(etag)) store(key, body, etag);
    return { body, cache: "MISS" };
  } catch {
    remove(key);
    return { status: 503 };
  }
}

export async function readPublicGitObject(request, upstream) {
  const headers = { "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" };
  const error = status => Response.json({ error: "Public Git object unavailable." }, { status, headers });
  if (request.method !== "GET") return error(405);
  const target = targetFor(new URL(request.url));
  if (!target) return error(404);
  if (Date.now() < pausedUntil) return error(429);
  let pending = inFlight.get(target.href);
  if (!pending) {
    if (inFlight.size >= maxInFlight) return error(503);
    pending = fetchPublic(target, upstream).finally(() => inFlight.delete(target.href));
    inFlight.set(target.href, pending);
  }
  const result = await pending;
  if (!result.body) return error(result.status);
  const body = new ReadableStream({
    start(controller) {
      controller.enqueue(result.body);
      controller.close();
    },
  });
  return new Response(body, { headers: {
    ...headers, "Content-Type": "application/json", "X-Repo-City-Cache": result.cache,
  } });
}
