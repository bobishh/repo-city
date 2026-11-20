import assert from "node:assert/strict";
import { test } from "node:test";
import { handleGitHub } from "./github-auth.mjs";
import { loadGitHub } from "./github.mjs";

const origin = "https://repo-city.example";
const sha = "a".repeat(40);
const object = repo => `/github/public/repos/owner/${repo}/git/blobs/${sha}`;
const request = (path, headers = {}) => new Request(origin + path, { headers });
const publicBlob = (body, etag = '"public-v1"') => Response.json(body, {
  headers: { ETag: etag, "Cache-Control": "public, max-age=60" },
});
const unchanged = () => new Response(null, {
  status: 304, headers: { "Cache-Control": "public, max-age=60" },
});

test("one public fetch serves concurrent browsers, and each later hit revalidates without credentials", async () => {
  const path = object("visible");
  const body = { sha, content: "published data" };
  const seen = [];
  const upstream = async (url, options) => {
    const headers = new Headers(options.headers);
    assert.equal(url.href, `https://api.github.com/repos/owner/visible/git/blobs/${sha}`);
    assert.equal(headers.get("Authorization"), null);
    assert.equal(headers.get("Cookie"), null);
    assert.equal(options.redirect, "manual");
    seen.push(headers.get("If-None-Match"));
    if (seen.length === 1) {
      await new Promise(resolve => setImmediate(resolve));
      return publicBlob(body);
    }
    return unchanged();
  };
  const load = () => handleGitHub(request(path, {
    Cookie: "__Host-repo-city=untrusted-cookie", Authorization: "Bearer untrusted-token",
    "If-None-Match": '"untrusted-validator"',
  }), {}, upstream);

  const pair = await Promise.all([load(), load()]);
  assert.deepEqual(await Promise.all(pair.map(response => response.json())), [body, body]);
  assert.deepEqual(seen, [null]);
  assert.deepEqual(await (await load()).json(), body);
  assert.deepEqual(seen, [null, '"public-v1"']);
});

test("an object that becomes private is evicted and its saved bytes never match a different repository", async () => {
  const seen = [];
  let visibility = "public";
  const upstream = async (url, options) => {
    seen.push([url.pathname, new Headers(options.headers).get("If-None-Match")]);
    if (url.pathname.includes("/other/") || visibility === "private") {
      return Response.json({ internal: "private error details" }, { status: 404 });
    }
    if (visibility === "failure") return new Response(null, { status: 503 });
    return publicBlob({ content: "public data" });
  };
  const load = repo => handleGitHub(request(object(repo)), {}, upstream);
  assert.equal((await load("revoked")).status, 200);
  assert.equal((await load("other")).status, 404);
  visibility = "private";
  const denied = await load("revoked");
  assert.equal(denied.status, 404);
  assert.doesNotMatch(await denied.text(), /public data|private error details/);
  visibility = "public";
  await load("revoked");
  visibility = "failure";
  assert.equal((await load("revoked")).status, 503);
  visibility = "public";
  await load("revoked");
  assert.deepEqual(seen.map(([, tag]) => tag), [null, null, '"public-v1"', null, '"public-v1"', null]);
});

test("private responses, unexpected redirects and new route shapes cannot populate the shared cache", async () => {
  const path = object("guarded");
  const seen = [];
  const upstream = async (_url, options) => {
    seen.push(new Headers(options.headers).get("If-None-Match"));
    if (seen.length === 1) return Response.json({ content: "private response" }, {
      headers: { ETag: '"private"', "Cache-Control": "private, max-age=60" },
    });
    if (seen.length === 2) return Response.redirect("https://api.github.com/repositories/42", 301);
    return publicBlob({ content: "public response" });
  };
  for (let i = 0; i < 2; i++) {
    const response = await handleGitHub(request(path), {}, upstream);
    assert.notEqual(response.status, 200);
    assert.doesNotMatch(await response.text(), /private response/);
  }
  assert.equal((await handleGitHub(request(path), {}, upstream)).status, 200);
  assert.deepEqual(seen, [null, null, null]);
  for (const invalid of [path + "?recursive=1", "/github/public/user/starred", "/github/public/repos/owner/guarded/pulls/1"]) {
    const response = await handleGitHub(request(invalid), {}, () => assert.fail("Unsupported requests must stay local"));
    assert.equal(response.status, 404);
  }
});

test("the viewer selects the shared route only for a known public repository or fork", async () => {
  const seen = [];
  const blobSha = "b".repeat(40);
  const forkBlobSha = "c".repeat(40);
  const request = async (url, options) => {
    const path = new URL(url).pathname;
    if (path.endsWith("/pulls/8")) return Response.json({
      head: { sha, repo: { full_name: "fork/project", private: false, visibility: "public" } },
      base: { sha: blobSha, repo: { full_name: "owner/project", private: true } },
    });
    if (path.includes("/compare/")) return Response.json({ merge_base_commit: { sha: blobSha } });
    seen.push([path, Boolean(options.publicObject)]);
    if (path.includes("/git/trees/")) return Response.json({
      tree: [{ path: "file.rs", type: "blob", mode: "100644", sha: path.includes("/fork/") ? forkBlobSha : blobSha, size: 2 }],
    });
    return Response.json({ encoding: "base64", content: "YQo=", size: 2 });
  };
  const repository = await loadGitHub("https://github.com/owner/project/pull/8", null, null, request);
  await repository.blob(forkBlobSha);
  await repository.blob(blobSha);
  assert.deepEqual(seen, [
    [`/repos/owner/project/git/trees/${blobSha}`, false],
    [`/repos/fork/project/git/trees/${sha}`, true],
    [`/repos/fork/project/git/blobs/${forkBlobSha}`, true],
    [`/repos/owner/project/git/blobs/${blobSha}`, false],
  ]);
});

test("an upstream quota failure clears access to a warm entry and suppresses more anonymous calls", async () => {
  const path = object("quota");
  let count = 0;
  const upstream = async () => ++count === 1
    ? publicBlob({ content: "already cached" })
    : new Response(null, { status: 403, headers: { "X-RateLimit-Remaining": "0" } });
  assert.equal((await handleGitHub(request(path), {}, upstream)).status, 200);
  const limited = await handleGitHub(request(path), {}, upstream);
  assert.equal(limited.status, 403);
  assert.doesNotMatch(await limited.text(), /already cached/);
  assert.equal((await handleGitHub(request(path), {}, upstream)).status, 429);
  assert.equal(count, 2);
});
