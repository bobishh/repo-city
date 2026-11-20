import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { get } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { gzipSync } from "node:zlib";
import { handleGitHub } from "./github-auth.mjs";
import { startServer } from "./server.mjs";

const origin = "https://repo-city.meta-uber-engineer.dev";
const wasm = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]);
const env = {
  GITHUB_CLIENT_ID: "client-id",
  GITHUB_CLIENT_SECRET: "test-only-secret",
  REPO_CITY_REVISION: "fixture",
};

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), "repo-city-server-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(join(root, "game"));
  await writeFile(join(root, "game/repo_city_bg.wasm.gz"), gzipSync(wasm));
  const server = startServer({ port: 0, host: "127.0.0.1", origin, env, root });
  t.after(() => new Promise(resolve => server.close(resolve)));
  await once(server, "listening");
  return (path, headers = {}) => new Promise((resolve, reject) => {
    const request = get(`http://127.0.0.1:${server.address().port}${path}`, { headers }, async response => {
      try {
        const chunks = [];
        for await (const chunk of response) chunks.push(chunk);
        resolve({ status: response.statusCode, headers: response.headers, body: Buffer.concat(chunks) });
      } catch (error) { reject(error); }
    });
    request.on("error", reject);
  });
}

test("the VPS serves the prebuilt gzip WASM and an identity response on the same origin", async t => {
  const request = await fixture(t);
  const health = await request("/up");
  assert.equal(health.status, 200);
  assert.equal(health.headers["x-repo-city-backend"], "vps");
  assert.equal(health.headers["x-repo-city-revision"], "fixture");
  const compressed = await request("/game/repo_city_bg.wasm", { "Accept-Encoding": "br, gzip" });
  assert.equal(compressed.status, 200);
  assert.equal(compressed.headers["content-type"], "application/wasm");
  assert.equal(compressed.headers["content-encoding"], "gzip");
  assert.equal(compressed.headers.vary, "Accept-Encoding");
  assert.deepEqual(compressed.body, gzipSync(wasm));
  const identity = await request("/game/repo_city_bg.wasm", { "Accept-Encoding": "gzip;q=0, identity" });
  assert.equal(identity.headers["content-encoding"], undefined);
  assert.deepEqual(identity.body, wasm);
  assert.equal((await request("/game/missing.wasm")).status, 404);
  assert.equal(JSON.parse((await request("/party/config")).body).enabled, false);
});

test("OAuth uses the public HTTPS origin and can read a flow cookie issued by the old handler", async t => {
  const request = await fixture(t);
  const login = await request("/github/login", { Host: "proxy.local:8080" });
  assert.equal(login.status, 303);
  const url = new URL(login.headers.location);
  assert.equal(url.origin, "https://github.com");
  assert.equal(url.searchParams.get("redirect_uri"), `${origin}/github/callback`);
  assert.match(login.headers["set-cookie"][0], /^__Host-repo-city-oauth=.*; Path=\/; Secure; HttpOnly;/);

  const old = await handleGitHub(new Request(`${origin}/github/login?return=%2F%3Fmigrate%3D1`), env);
  const cookie = old.headers.getSetCookie()[0].split(";", 1)[0];
  const resumed = await request("/github/callback", { Cookie: cookie });
  assert.equal(resumed.headers.location, "/?migrate=1&github=failed");
  assert.deepEqual(JSON.parse((await request("/github/session")).body), { enabled: true, login: null, installUrl: null });
});
