import assert from "node:assert/strict";
import { createHash } from "node:crypto";

const [base, revision, parties = "false"] = process.argv.slice(2);
const origin = "https://repo-city.meta-uber-engineer.dev";

async function check(path, status = 200, headers = {}, method = "GET") {
  const response = await fetch(new URL(path, base), {
    headers, method, redirect: "manual", signal: AbortSignal.timeout(25_000),
  });
  assert.equal(response.status, status, path);
  assert.equal(response.headers.get("x-repo-city-backend"), "vps", path);
  assert.equal(response.headers.get("x-repo-city-revision"), revision, path);
  return response;
}

await (await check("/up")).arrayBuffer();
const html = await (await check("/")).text();
assert.equal(html.includes('id="deployment-browser-script"'), Boolean(process.env.REPO_CITY_BROWSER_SCRIPT),
  "The served index matches the deployment's browser script configuration");
const build = html.match(/data-engine-build="([a-f0-9]{24})"/)?.[1];
assert.ok(build, "Engine fingerprint in HTML");
assert.ok(html.includes(`app.mjs?v=${build}`), "Entrypoint uses the selected engine build");
const game = `/game/${build}`;
const headers = { "Accept-Encoding": "gzip" };
const canonical = await check("/game/repo_city_bg.wasm", 200, headers, "HEAD");
assert.equal(canonical.headers.get("content-encoding"), "gzip");
const wasm = await check(`${game}/repo_city_bg.wasm`, 200, headers);
assert.equal(wasm.headers.get("content-type"), "application/wasm");
assert.equal(wasm.headers.get("content-encoding"), "gzip");
const bytes = Buffer.from(await wasm.arrayBuffer());
assert.deepEqual(bytes.subarray(0, 4), Buffer.from([0, 97, 115, 109]));
const glue = await (await check(`${game}/repo_city.js`)).arrayBuffer();
const shader = await (await check(`${game}/webgpu-shader.mjs`)).arrayBuffer();
const fingerprint = createHash("sha256").update(Buffer.from(glue)).update(bytes).update(Buffer.from(shader));
assert.equal(fingerprint.digest("hex").slice(0, 24), build, "Engine files match their URL fingerprint");
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
