import assert from "node:assert/strict";
import { test } from "node:test";
import { handleGitHub } from "./github-auth.mjs";

const site = "https://repo-city.example";
const env = { GITHUB_CLIENT_ID: "test-id", GITHUB_CLIENT_SECRET: "test-secret", GITHUB_APP_SLUG: "repo-city-viewer" };
const request = (path, options) => new Request(site + path, options);

test("OAuth uses PKCE, binds state to the browser, and keeps the token in its encrypted cookie", async () => {
  const login = await handleGitHub(request("/github/login?return=%2F%3Fpicker%3D1"), env);
  assert.equal(login.status, 303);
  const authorization = new URL(login.headers.get("Location"));
  assert.equal(authorization.hostname, "github.com");
  assert.equal(authorization.searchParams.get("code_challenge_method"), "S256");
  const flow = login.headers.get("Set-Cookie").split(";")[0];
  const state = authorization.searchParams.get("state");
  assert.equal((await handleGitHub(request("/github/callback?code=example&state=bad", {
    headers: { Cookie: flow },
  }), env, () => assert.fail("Invalid state must not be sent to GitHub"))).status, 303);
  const token = "ghu_test-only";
  const callback = await handleGitHub(request("/github/callback?code=example&state=" + state, {
    headers: { Cookie: flow },
  }), env, async (url, options) => {
    assert.equal(options.redirect, "manual");
    if (String(url).endsWith("/access_token")) {
      const verifier = options.body.get("code_verifier");
      const digest = Buffer.from(await crypto.subtle.digest("SHA-256", Buffer.from(verifier))).toString("base64url");
      assert.equal(digest, authorization.searchParams.get("code_challenge"));
      return Response.json({ access_token: token, expires_in: 3600 });
    }
    assert.equal(options.headers.Authorization, "Bearer " + token);
    return Response.json({ login: "owner" });
  });
  assert.equal(callback.headers.get("Location"), "/?picker=1");
  const session = callback.headers.getSetCookie().find(value => value.startsWith("__Host-repo-city="));
  assert.ok(session.includes("HttpOnly") && session.includes("Secure") && !session.includes(token));
  const headers = { Cookie: session.split(";")[0] };
  assert.deepEqual(await (await handleGitHub(request("/github/session", { headers }), env)).json(), {
    enabled: true, login: "owner", installUrl: "https://github.com/apps/repo-city-viewer/installations/new",
  });

  const allowed = await handleGitHub(request("/github/api/repos/owner/private/branches?per_page=100", { headers }), env, async (url, options) => {
    assert.equal(String(url), "https://api.github.com/repos/owner/private/branches?per_page=100");
    assert.equal(options.headers.Authorization, "Bearer " + token);
    return Response.json([{ name: "private-branch" }]);
  });
  assert.match(allowed.headers.get("Cache-Control"), /no-store/);
  assert.equal((await allowed.json())[0].name, "private-branch");
  const redirects = [];
  const renamed = await handleGitHub(request("/github/api/repos/owner/old-name", { headers }), env, async (url, options) => {
    assert.equal(options.redirect, "manual");
    redirects.push(String(url));
    return redirects.length === 1
      ? Response.redirect("https://api.github.com/repositories/123", 301)
      : Response.redirect("https://example.org/", 302);
  });
  assert.equal(renamed.status, 404);
  assert.deepEqual(redirects, ["https://api.github.com/repos/owner/old-name", "https://api.github.com/repositories/123"]);
  assert.equal((await handleGitHub(request("/github/api/repos/owner/private/commits/..%2F..%2F..%2Fuser", { headers }), env)).status, 404);
  assert.equal((await handleGitHub(request("/github/api/user/emails", { headers }), env,
    () => assert.fail("Unlisted endpoints must never be sent to GitHub"))).status, 404);
  assert.equal((await handleGitHub(request("/github/api/repos/owner/private", { method: "POST", headers }), env)).status, 405);
  assert.equal((await handleGitHub(request("/github/logout", { method: "POST", headers }), env)).status, 403);
  assert.equal((await handleGitHub(request("/github/logout", { method: "POST", headers: { ...headers, Origin: site } }), env)).status, 204);
});

test("OAuth remains optional and rejects requests for private data without a session", async () => {
  assert.equal((await (await handleGitHub(request("/github/session"), {})).json()).enabled, false);
  assert.equal((await handleGitHub(request("/github/api/repos/owner/private"), env)).status, 401);
});

test("OAuth upstream failures and redirects return to the picker and clear the flow", async () => {
  for (const upstream of [
    async () => { throw new TypeError("Connection reset"); },
    async () => Response.redirect("https://example.org/", 302),
  ]) {
    const login = await handleGitHub(request("/github/login?return=%2F%3Fpicker%3D1%26repo%3Downer%252Fproject"), env);
    const state = new URL(login.headers.get("Location")).searchParams.get("state");
    const flow = login.headers.get("Set-Cookie").split(";")[0];
    const callback = await handleGitHub(request("/github/callback?code=example&state=" + state, {
      headers: { Cookie: flow },
    }), env, upstream);
    assert.equal(callback.status, 303);
    assert.equal(callback.headers.get("Location"), "/?picker=1&repo=owner%2Fproject&github=failed");
    assert.match(callback.headers.get("Set-Cookie"), /^__Host-repo-city-oauth=;.*Max-Age=0$/);
  }
});
