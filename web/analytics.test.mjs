import assert from "node:assert/strict";
import { test } from "node:test";
import { createAnalytics } from "./analytics.mjs";

const origin = "https://repo-city.meta-uber-engineer.dev";
function fixture(overrides = {}) {
  const requests = [];
  const browser = {
    location: new URL(`${origin}/?repo=private-marker/prototype&ref=draft&base=main#marker`),
    navigator: {},
    crypto: { randomUUID: () => "683f30d5-8118-4594-bc7e-f645763d769d" },
    fetch: (url, options) => { requests.push({ url, options }); return Promise.resolve(); },
    ...overrides,
  };
  return { requests, browser };
}

test("sends a parameter-free page view and bounded repository outcomes", () => {
  const { requests, browser } = fixture();
  const analytics = createAnalytics("project-browser-key", browser);
  analytics.pageView();
  analytics.repositoryOpened(true, 135.5, true);
  analytics.repositoryOpened(false, 200, false);
  assert.equal(requests.length, 3);
  const events = requests.map(({ url, options }) => {
    assert.equal(url, "https://analytics.meta-uber-engineer.dev/events");
    assert.equal(options.credentials, "omit");
    assert.equal(options.referrerPolicy, "no-referrer");
    assert.equal(options.headers.Authorization, "Bearer project-browser-key");
    assert.doesNotMatch(options.body, /private-marker|prototype|draft|main|repo=|#marker/);
    const { project, source, events } = JSON.parse(options.body);
    assert.equal(project, "repo-city");
    assert.equal(source, "browser");
    return events[0];
  });
  assert.equal(events[0].path, "/");
  assert.equal(events[0].event, "page_view");
  assert.equal(events[1].event, "repo_city.repository.open");
  assert.deepEqual(events[1].attrs, { mode: "comparison" });
  assert.equal(events[1].status, "ok");
  assert.equal(events[2].status, "error");
});

test("previews, local development, missing keys and privacy signals disable collection", () => {
  for (const [key, overrides] of [
    ["", {}],
    ["key", { location: new URL("http://127.0.0.1:19401/") }],
    ["key", { location: new URL(`${origin}/?press-shot`) }],
    ["key", { navigator: { doNotTrack: "1" } }],
    ["key", { navigator: { globalPrivacyControl: true } }],
  ]) {
    const { requests, browser } = fixture(overrides);
    const analytics = createAnalytics(key, browser);
    analytics.pageView();
    analytics.repositoryOpened(true, 1, false);
    assert.deepEqual(requests, []);
  }
});

test("a failed telemetry request never interrupts a repository opening", async () => {
  for (const fetch of [() => { throw Error("blocked"); }, () => Promise.reject(Error("blocked"))]) {
    const { browser } = fixture({ fetch });
    const analytics = createAnalytics("key", browser);
    assert.doesNotThrow(() => analytics.pageView());
    assert.doesNotThrow(() => analytics.repositoryOpened(true, 1, false));
  }
});

test("tracks tour outcomes and GitHub connection without sending account or source data", () => {
  const { requests, browser } = fixture();
  const analytics = createAnalytics("project-browser-key", browser);
  analytics.tour("start", "first_run");
  analytics.tour("complete", "help");
  analytics.tour("skip", "help");
  analytics.tour("private-needle", "help");
  analytics.tour("start", "private-repo");
  analytics.githubConnected();
  const events = requests.map(({ options }) => JSON.parse(options.body).events[0]);
  assert.deepEqual(events.map(({ event, operation, attrs }) => [event, operation, attrs]), [
    ["repo_city.tour", "start", { entry: "first_run" }],
    ["repo_city.tour", "complete", { entry: "help" }],
    ["repo_city.tour", "skip", { entry: "help" }],
    ["repo_city.github.connect", "connect", {}],
  ]);
  for (const request of requests) assert.doesNotMatch(request.options.body, /private|prototype|repo=|#marker/);
});
