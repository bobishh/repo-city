// The Worker holds GitHub credentials and forwards only read requests used by the city.
const API = "https://api.github.com";
const SESSION = "__Host-repo-city";
const FLOW = "__Host-repo-city-oauth";
const utf8 = new TextEncoder();
const noStore = { "Cache-Control": "private, no-store", "Referrer-Policy": "no-referrer" };

const encode = bytes => btoa(String.fromCharCode(...bytes)).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
const decode = value => Uint8Array.from(atob(value.replaceAll("-", "+").replaceAll("_", "/")), char => char.charCodeAt(0));
const random = () => encode(crypto.getRandomValues(new Uint8Array(32)));
const digest = async value => new Uint8Array(await crypto.subtle.digest("SHA-256", utf8.encode(value)));
const key = async secret => crypto.subtle.importKey(
  "raw", await digest("repo-city:github:v1:" + secret), "AES-GCM", false, ["encrypt", "decrypt"],
);

async function readCookie(request, name, secret) {
  const cookies = request.headers.get("Cookie") ?? "";
  const value = cookies.split("; ").find(cookie => cookie.startsWith(name + "="))?.slice(name.length + 1);
  if (!value || value.length > 3600) return null;
  try {
    const [iv, ciphertext] = value.split(".");
    const data = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: decode(iv), additionalData: utf8.encode(name) },
      await key(secret), decode(ciphertext),
    );
    const payload = JSON.parse(new TextDecoder().decode(data));
    return payload.expires > Date.now() ? payload : null;
  } catch {
    return null;
  }
}

function cookie(name, value, seconds) {
  return `${name}=${value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=${seconds}`;
}

async function setCookie(name, payload, seconds, secret) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const data = utf8.encode(JSON.stringify({ ...payload, expires: Date.now() + seconds * 1000 }));
  const encrypted = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv, additionalData: utf8.encode(name) }, await key(secret), data,
  );
  return cookie(name, encode(iv) + "." + encode(new Uint8Array(encrypted)), seconds);
}

function json(value, status = 200) {
  return Response.json(value, { status, headers: noStore });
}

function redirect(location, cookies = []) {
  const headers = new Headers({ ...noStore, Location: location });
  for (const value of cookies) headers.append("Set-Cookie", value);
  return new Response(null, { status: 303, headers });
}

function returnPath(value, origin) {
  if (!value?.startsWith("/") || value.startsWith("//") || value.length > 1200) return "/";
  const url = new URL(value, origin);
  return url.origin === origin && url.pathname === "/" ? url.pathname + url.search : "/";
}

function endpoint(url) {
  const path = url.pathname.slice("/github/api".length);
  if (path === "/search/commits") {
    const q = url.searchParams.get("q");
    // Only accept one repository qualifier followed by literal keyword terms.
    if (!q || q.length > 220 || !/^repo:[\w.-]+\/[\w.-]+(?: "[\p{L}\p{N}_-]+"){1,8}$/u.test(q)) return null;
    return new URL(path + "?" + new URLSearchParams({ q, per_page: "8" }), API);
  }
  const repo = /^\/repos\/[\w.-]+\/[\w.-]+(?:\/(?:branches|tags|commits|commits\/[\w.%~-]+|compare\/[\w.%~:-]+|pulls\/[1-9]\d*|git\/(?:trees|blobs)\/[a-f\d]{40,64}))?$/i;
  const account = /^\/(?:user\/(?:starred|installations(?:\/\d+\/repositories)?)|users\/[\w-]+\/repos)$/;
  if (!repo.test(path) && !account.test(path)) return null;
  const decoded = decodeURIComponent(path);
  if (decoded.includes("\x5c") || decoded.split("/").some(part => part === "." || part === "..")) return null;
  const target = new URL(path, API);
  for (const name of ["per_page", "page", "recursive", "sort"]) {
    if (!url.searchParams.has(name)) continue;
    const value = url.searchParams.get(name);
    if (name === "sort" ? value !== "pushed" : !/^\d{1,3}$/.test(value)) return null;
    target.searchParams.set(name, value);
  }
  return target;
}

function github(token) {
  return {
    Accept: "application/vnd.github+json",
    Authorization: `Bearer ${token}`,
    "User-Agent": "Repo City",
    "X-GitHub-Api-Version": "2022-11-28",
  };
}

export async function handleGitHub(request, env, upstream = fetch) {
  const url = new URL(request.url);
  const path = url.pathname;
  const { GITHUB_CLIENT_ID: id, GITHUB_CLIENT_SECRET: secret, GITHUB_APP_SLUG: slug } = env;
  const enabled = Boolean(id && secret);
  try {
    if (path === "/github/session" && request.method === "GET") {
      const session = enabled ? await readCookie(request, SESSION, secret) : null;
      const installUrl = slug && /^[a-z\d-]+$/.test(slug) ? `https://github.com/apps/${slug}/installations/new` : null;
      return json({ enabled, login: session?.login ?? null, installUrl });
    }
    if (!enabled) return json({ error: "GitHub sign-in is unavailable." }, 503);
    if (path === "/github/blame" && request.method === "GET") {
      const session = await readCookie(request, SESSION, secret);
      if (!session) return json({ error: "Sign in with GitHub." }, 401);
      const owner = url.searchParams.get("owner");
      const name = url.searchParams.get("repo");
      const oid = url.searchParams.get("sha");
      const file = url.searchParams.get("path");
      const validName = value => /^[\w.-]{1,100}$/.test(value ?? "") && value !== "." && value !== "..";
      if (!validName(owner) || !validName(name) || !/^[a-f\d]{40}$/i.test(oid ?? "") ||
          !file || file.length > 4096 || /[\u0000-\u001f]/u.test(file) ||
          file.split("/").some(part => !part || part === "." || part === "..")) {
        return json({ error: "Invalid blame target." }, 400);
      }
      const response = await upstream(API + "/graphql", {
        method: "POST",
        headers: { ...github(session.token), "Content-Type": "application/json" },
        redirect: "manual",
        body: JSON.stringify({
          query: `query CityBlame($owner: String!, $name: String!, $oid: GitObjectID!, $path: String!) {
            repository(owner: $owner, name: $name) {
              object(oid: $oid) {
                ... on Commit {
                  blame(path: $path) {
                    ranges { startingLine endingLine commit { abbreviatedOid committedDate author { name user { login } } } }
                  }
                }
              }
            }
          }`,
          variables: { owner, name, oid, path: file },
        }),
      });
      if (response.status === 401) return relay(response);
      if (!response.ok) return json({ error: "Line history unavailable." }, response.status === 403 ? 403 : 502);
      const ranges = (await response.json()).data?.repository?.object?.blame?.ranges;
      return Array.isArray(ranges) ? json({ ranges }) : json({ error: "Line history unavailable." }, 404);
    }
    if (path === "/github/logout" && request.method === "POST") {
      if (request.headers.get("Origin") !== url.origin) return json({ error: "Invalid origin." }, 403);
      return new Response(null, {
        status: 204, headers: { ...noStore, "Set-Cookie": cookie(SESSION, "", 0) },
      });
    }
    if (request.method !== "GET") return json({ error: "Method not allowed." }, 405);
    const callback = url.origin + "/github/callback";
    if (path === "/github/login") {
      const verifier = random();
      const state = random();
      const params = new URLSearchParams({
        client_id: id, redirect_uri: callback, state,
        code_challenge: encode(await digest(verifier)), code_challenge_method: "S256",
      });
      const returnTo = returnPath(url.searchParams.get("return"), url.origin);
      return redirect("https://github.com/login/oauth/authorize?" + params, [
        await setCookie(FLOW, { verifier, state, returnTo }, 600, secret),
      ]);
    }
    if (path === "/github/callback") {
      const flow = await readCookie(request, FLOW, secret);
      const clear = cookie(FLOW, "", 0);
      const code = url.searchParams.get("code");
      const failure = new URL(flow?.returnTo ?? "/", url.origin);
      failure.searchParams.set("github", "failed");
      const failed = () => redirect(failure.pathname + failure.search, [clear]);
      if (!flow || !code || url.searchParams.get("state") !== flow.state) return failed();
      try {
        const exchange = await upstream("https://github.com/login/oauth/access_token", {
          method: "POST",
          headers: { Accept: "application/json" },
          body: new URLSearchParams({
            client_id: id, client_secret: secret, code, redirect_uri: callback, code_verifier: flow.verifier,
          }),
          redirect: "manual",
        });
        if (!exchange.ok) return failed();
        const { access_token: token, expires_in: expires } = await exchange.json();
        if (typeof token !== "string" || !token.startsWith("ghu_")) return failed();
        const profile = await upstream(API + "/user", { headers: github(token), redirect: "manual" });
        if (!profile.ok) return failed();
        const { login } = await profile.json();
        if (typeof login !== "string" || !/^[\w-]+$/.test(login)) return failed();
        const age = Math.max(1, Math.min(28_800, Number(expires) || 28_800));
        return redirect(flow.returnTo, [clear, await setCookie(SESSION, { token, login }, age, secret)]);
      } catch {
        return failed();
      }
    }
    const target = path.startsWith("/github/api/") && endpoint(url);
    if (!target) return json({ error: "Unknown GitHub route." }, 404);
    const session = await readCookie(request, SESSION, secret);
    if (!session) return json({ error: "Sign in with GitHub." }, 401);
    const response = await upstream(target, { headers: github(session.token), redirect: "manual" });
    if (response.status >= 300 && response.status < 400) {
      // GitHub can redirect requests for a renamed repository to its immutable API ID.
      const location = response.headers.get("location");
      const next = location && new URL(location, API);
      if (!next || next.origin !== API || !/^\/repositories\/\d+(?:\/|$)/.test(next.pathname)) {
        return json({ error: "GitHub redirected this request." }, 404);
      }
      await response.body?.cancel();
      const renamed = await upstream(next, { headers: github(session.token), redirect: "manual" });
      if (renamed.status >= 300 && renamed.status < 400) {
        await renamed.body?.cancel();
        return json({ error: "GitHub redirected this request." }, 404);
      }
      return relay(renamed);
    }
    return relay(response);
  } catch {
    return json({ error: "Could not reach GitHub." }, 502);
  }
}

function relay(response) {
  const headers = new Headers({ ...noStore, "Content-Type": "application/json", "X-Content-Type-Options": "nosniff" });
  const link = response.headers.get("Link");
  if (link) headers.set("Link", link);
  if (response.status === 401) headers.set("Set-Cookie", cookie(SESSION, "", 0));
  return new Response(response.body, { status: response.status, headers });
}
