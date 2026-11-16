// TinSink v1 analytics and v2 telemetry, following the Paket browser policy.
const ORIGIN = "https://repo-city.meta-uber-engineer.dev";
const ENDPOINT = "https://analytics.meta-uber-engineer.dev/events";
const PROJECT = "repo-city";

export function createAnalytics(browserKey, browser = window) {
  const enabled = Boolean(browserKey)
    && browser.location.origin === ORIGIN
    && !new URLSearchParams(browser.location.search).has("press-shot")
    && browser.navigator.doNotTrack !== "1"
    && !browser.navigator.globalPrivacyControl;

  function send(schema, fields) {
    if (!enabled) return;
    try {
      const body = JSON.stringify({
        schema_version: schema,
        ...(schema === 2 ? { stream: "telemetry" } : {}),
        project: PROJECT,
        source: "browser",
        events: [{
          event_id: browser.crypto.randomUUID(),
          occurred_at: new Date().toISOString(),
          ...fields,
        }],
      });
      void browser.fetch(ENDPOINT, {
        method: "POST",
        credentials: "omit",
        referrerPolicy: "no-referrer",
        keepalive: true,
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${browserKey}` },
        body,
      }).catch(() => {});
    } catch { /* Unavailable telemetry must not interrupt the city. */ }
  }

  function telemetry(event, component, operation, status, duration, attrs) {
    send(2, {
      event, component, operation, status, attrs,
      build: "", session_id: "", device_id: "", workspace_id: "", entity_type: "", entity_id: "",
      trace_id: "", span_id: "", parent_span_id: "",
      duration_ms: Number.isFinite(duration) ? Math.min(86_400_000, Math.max(0, duration)) : 0,
    });
  }

  return {
    pageView() {
      send(1, { event: "page_view", path: "/", target: "", referrer_host: "", properties: {} });
    },
    repositoryOpened(ok, elapsedMs, comparing) {
      telemetry("repo_city.repository.open", "repository", "open", ok ? "ok" : "error",
        elapsedMs, { mode: comparing ? "comparison" : "snapshot" });
    },
    tour(action, entry) {
      if (!["start", "complete", "skip"].includes(action) || !["first_run", "help"].includes(entry)) return;
      telemetry("repo_city.tour", "onboarding", action, "ok", 0, { entry });
    },
    githubConnected() {
      telemetry("repo_city.github.connect", "github", "connect", "ok", 0, {});
    },
  };
}
