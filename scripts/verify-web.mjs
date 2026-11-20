import assert from "node:assert/strict";

const [base, revision] = process.argv.slice(2);
const origin = "https://repo-city.meta-uber-engineer.dev";

async function check(path, status = 200, headers = {}) {
  const response = await fetch(new URL(path, base), {
    headers, redirect: "manual", signal: AbortSignal.timeout(25_000),
  });
  assert.equal(response.status, status, path);
  assert.equal(response.headers.get("x-repo-city-backend"), "vps", path);
  assert.equal(response.headers.get("x-repo-city-revision"), revision, path);
  return response;
}

await (await check("/up")).arrayBuffer();
await (await check("/")).arrayBuffer();
const wasm = await check("/game/repo_city_bg.wasm", 200, { "Accept-Encoding": "gzip" });
assert.equal(wasm.headers.get("content-type"), "application/wasm");
assert.equal(wasm.headers.get("content-encoding"), "gzip");
assert.deepEqual(new Uint8Array((await wasm.arrayBuffer()).slice(0, 4)), Uint8Array.of(0, 97, 115, 109));
assert.equal((await (await check("/github/session")).json()).enabled, true);
assert.equal((await (await check("/party/config")).json()).enabled, false);
const login = await check("/github/login", 303);
const authorization = new URL(login.headers.get("location"));
assert.equal(authorization.origin, "https://github.com");
assert.equal(authorization.searchParams.get("redirect_uri"), `${origin}/github/callback`);
assert.match(login.headers.getSetCookie()[0], /^__Host-repo-city-oauth=.*; Path=\/; Secure; HttpOnly;/);
console.log("Verified the VPS revision, assets, gzip WASM, GitHub redirect and disabled rooms.");
