import assert from "node:assert/strict";

const [base, revision, parties = "false"] = process.argv.slice(2);
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
assert.deepEqual(await (await check("/party/config")).json(), {enabled: parties === "true", layout_version: 1});
if (parties === "true") await (await check("/party/rooms/00000000-0000-4000-8000-000000000000/events", 403)).arrayBuffer();
const login = await check("/github/login", 303);
const authorization = new URL(login.headers.get("location"));
assert.equal(authorization.origin, "https://github.com");
assert.equal(authorization.searchParams.get("redirect_uri"), `${origin}/github/callback`);
const [flowCookie] = login.headers.getSetCookie();
assert.match(flowCookie, /^__Host-repo-city-oauth=[^;]+/);
const attributes = flowCookie.split(";").slice(1).map(part => part.trim().toLowerCase());
for (const required of ["path=/", "secure", "httponly", "samesite=lax"]) assert.ok(attributes.includes(required), required);
assert.ok(!attributes.some(attribute => attribute.startsWith("domain=")));
console.log("Verified the VPS revision, assets, gzip WASM, GitHub redirect and room availability.");
