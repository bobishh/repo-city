import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { createCipheriv, createHash, randomBytes } from "node:crypto";

async function modules(page) {
  for (const name of ["review-party-client.mjs", "review-party-ui.mjs"]) {
    const body = await readFile(new URL(`../${name}`, import.meta.url), "utf8");
    await page.route(`**/${name}`, route => route.fulfill({ contentType: "text/javascript", body }));
  }
}

test("party controls create an account-bound invite and follow only a selected view; denied microphone keeps room connected", async ({ page }) => {
  await modules(page);
  await page.route("**/party-client-fixture", route => route.fulfill({ contentType: "text/html", body: `<!doctype html><html lang="en"><style>
    body { color:#e4f2f2;background:#0a131a;font:16px system-ui;max-width:600px;padding:20px } button,input,fieldset { border-radius:0 } label { display:block } [hidden] { display:none }
  </style><main><h1>Party</h1><section id="party"></section></main><script type="module">
    import {createReviewParty} from '/review-party-client.mjs';
    import {mountReviewParty} from '/review-party-ui.mjs';
    const comparison={owner:'owner',repo:'project',head_sha:'a'.repeat(40),base_sha:'b'.repeat(40),layout_version:1};
    let socket;window.captures=0;window.jumps=[];
    class Socket {readyState=1;bufferedAmount=0;constructor(){socket=this;window.socket=this} send(){} close(){} }
    const client=createReviewParty({WebSocket:Socket,getUserMedia:async()=>{window.captures++;throw Error('Microphone permission denied')},
      fetch:async(url)=>({ok:true,json:async()=>url.endsWith('/invites')?{invite_url:'/?party=room&invite=secret'}:{room:{id:'room',host_id:'1',comparison},member:{id:'1',login:'alice'},socket_url:'/socket'}}),
      navigate:anchor=>window.jumps.push(anchor),loadComparison:async()=>{}});
    window.client=client;mountReviewParty(document.querySelector('#party'),client,{comparison:()=>comparison,clipboard:async()=>{}});
    window.deliver=message=>socket.onmessage({data:JSON.stringify(message)});
  </script></html>` }));
  await page.goto("/party-client-fixture");
  await page.getByRole("button", { name: "Create room", exact: true }).click();
  await expect(page.getByRole("status").first()).toHaveText("Connected");
  expect(await page.evaluate(() => window.captures)).toBe(0);
  await page.getByLabel("Invite GitHub account").fill("bob");
  await page.getByRole("button", { name: "Create invitation", exact: true }).click();
  await expect(page.getByLabel("Share invitation")).toHaveValue(/party=room&invite=secret/);
  await page.evaluate(() => window.deliver({type:"roster",members:[{id:"1",login:"alice"},{id:"2",login:"bob",focus:{path:"src/lib.rs",face:"blame",side:"before",line:16}}]}));
  expect(await page.evaluate(() => window.jumps)).toEqual([]);
  await page.getByRole("button", { name: "Fly to view", exact: true }).click();
  expect(await page.evaluate(() => window.jumps)).toEqual([{path:"src/lib.rs",face:"blame",before:true,line:16}]);
  await page.getByRole("button", { name: "Enable voice", exact: true }).click();
  await expect(page.getByRole("status").first()).toContainText("Microphone permission denied");
  await expect(page.getByRole("button", { name: "Leave room", exact: true })).toBeEnabled();
  await expect(page.getByLabel("Send consenting participants to my connected agent")).toBeDisabled();
  await page.getByRole("button", { name: "Leave room", exact: true }).click();
  await expect(page.getByRole("status").first()).toHaveText("Solo");
});

test("two browser members exchange real WebRTC audio while push-to-talk and leave close capture", async ({ page }) => {
  await modules(page);
  await page.goto("/");
  await page.evaluate(async () => {
    const { createReviewParty } = await import("/review-party-client.mjs");
    const comparison = { owner: "owner", repo: "project", head_sha: "a".repeat(40), base_sha: "b".repeat(40), layout_version: 1 };
    const sockets = new Map(), voices = new Map(), connections = [], captures = [];
    const roster = () => [...sockets.keys()].map(id => ({ id, login: `member${id}`, voice: voices.get(id) }));
    const deliver = (id, value) => sockets.get(id)?.onmessage?.({ data: JSON.stringify(value) });
    const broadcast = value => { for (const id of sockets.keys()) deliver(id, value); };
    const make = id => createReviewParty({
      WebSocket: class {
        readyState=1; bufferedAmount=0;
        constructor() { sockets.set(id, this); }
        send(raw) {
          const message = JSON.parse(raw);
          if (message.type === "voice") { voices.set(id, message); broadcast({ ...message, from: id }); }
          if (message.type === "signal") queueMicrotask(() => deliver(message.to, { ...message, from: id }));
        }
        close() { sockets.delete(id); }
      },
      RTCPeerConnection: class extends RTCPeerConnection { constructor(configuration) { super(configuration); connections.push(this); } },
      fetch: async () => ({ ok: true, json: async () => ({ room: { id:"room", host_id:"1", comparison }, member: { id, login:`member${id}` }, socket_url:"/socket", ice_servers:[] }) }),
      getUserMedia: async () => {
        const audio = new AudioContext(), oscillator = audio.createOscillator(), destination = audio.createMediaStreamDestination();
        oscillator.connect(destination); oscillator.start(); captures.push({ audio, oscillator, stream: destination.stream }); return destination.stream;
      },
      playRemote: () => ({ muted:false, play:async()=>{}, remove(){} }),
    });
    const first = make("1"), second = make("2");
    await first.create(comparison); await second.join("room", "token");
    broadcast({ type:"roster", members:roster() });
    await first.enableVoice(); await second.enableVoice(); first.talk(true);
    window.audioParty = { first, second, connections, captures };
  });
  await expect.poll(() => page.evaluate(async () => {
    const stats = await Promise.all(window.audioParty.connections.map(peer => peer.getStats()));
    return stats.flatMap(report => [...report.values()]).some(value => value.type === "inbound-rtp" && value.kind === "audio" && value.bytesReceived > 0);
  }), { timeout: 15_000 }).toBe(true);
  const result = await page.evaluate(async () => {
    const { first, second, connections, captures } = window.audioParty;
    first.talk(false); first.suspend(); await second.leave(); first.dispose();
    const ended = captures.every(({ stream }) => stream.getTracks().every(track => track.readyState === "ended"));
    const closed = connections.every(peer => peer.connectionState === "closed");
    await Promise.all(captures.map(({ audio }) => audio.close()));
    return { ended, closed };
  });
  expect(result).toEqual({ ended: true, closed: true });
});

function sessionCookie(origin, login) {
  const name = "__Host-repo-city";
  const key = createHash("sha256").update("repo-city:github:v1:test-secret").digest();
  const iv = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", key, iv).setAAD(Buffer.from(name));
  const payload = JSON.stringify({ login, token: "ghu_party_" + login, expires: Date.now() + 60_000 });
  const ciphertext = Buffer.concat([cipher.update(payload), cipher.final(), cipher.getAuthTag()]);
  return { name, value: iv.toString("base64url") + "." + ciphertext.toString("base64url"),
    url: new URL(origin).href.replace(/^http:/, "https:"), httpOnly: true, sameSite: "Lax" };
}

async function partyPage(browser, origin, login) {
  const context = await browser.newContext();
  await context.addCookies([sessionCookie(origin, login)]);
  const page = await context.newPage();
  const url = new URL("/party-screen", origin);
  if (process.env.REPO_CITY_TEST_RELAY === "1") url.searchParams.set("relay", "1");
  await page.goto(url.href);
  return { page, context };
}

test("Phoenix admits two browser sessions and routes WebRTC audio and review cues", async ({ browser, baseURL }) => {
  test.skip(process.env.REPO_CITY_TEST_PARTY !== "1", "Run against the Phoenix party fixture.");
  const host = await partyPage(browser, baseURL, "host");
  const guest = await partyPage(browser, baseURL, "guest");

  try {
    await host.page.getByRole("button", { name: "Create room", exact: true }).click();
    await expect(host.page.getByRole("status").first()).toHaveText("Connected");
    await host.page.getByLabel("Invite GitHub account").fill("guest");
    await host.page.getByRole("button", { name: "Create invitation", exact: true }).click();
    const value = host.page.getByLabel("Share invitation");
    await expect(value).toHaveValue(/party=[0-9a-f-]+#invite=rcparty_/);
    await guest.page.getByLabel("Invitation link").fill(await value.inputValue());
    await guest.page.getByRole("button", { name: "Join room", exact: true }).click();
    await expect(guest.page.getByRole("status").first()).toHaveText("Connected");
    await expect(host.page.getByRole("button", { name: "Remove guest" })).toBeVisible();
    expect(await guest.page.evaluate(() => window.pinned.head_sha)).toBe("a".repeat(40));
    expect(await guest.page.evaluate(() => window.party.sockets[0].protocol)).toBe("repo-city-party");

    const draft = host.page.getByRole("textbox", { name: "Room message" });
    await draft.fill("Review the last change.");
    const reject = route => route.fulfill({ status: 503, json: { error: "Chat temporarily unavailable." } });
    await host.page.route("**/party/rooms/*/events", reject);
    await host.page.getByRole("button", { name: "Send message", exact: true }).click();
    await expect(host.page.getByRole("status").first()).toContainText("Chat temporarily unavailable.");
    await expect(draft).toHaveValue("Review the last change.");
    await host.page.unroute("**/party/rooms/*/events", reject);
    await host.page.getByRole("button", { name: "Send message", exact: true }).click();
    await expect(guest.page.getByLabel("Room chat").getByText("Review the last change.", { exact: true })).toBeVisible();
    await expect(draft).toHaveValue("");

    await host.page.getByLabel("Sketch title").fill("Review flow");
    await host.page.getByLabel("Sketch format").selectOption("markdown");
    await host.page.getByLabel("Sketch source").fill("# Review\n\n```mermaid\nflowchart LR\n A[Change] --> B[Review]\n```");
    await host.page.getByRole("button", { name: "Share sketch", exact: true }).click();
    await expect(guest.page.getByLabel("Room sketches").getByText("Review flow", { exact: true })).toBeVisible();
    await expect(guest.page.getByRole("button", { name: "Share sketch", exact: true })).toBeHidden();

    await host.page.getByLabel("Share live reading focus").check();
    await host.page.evaluate(() => {
      window.party.client.update({ focus: {path:"src/lib.rs", face:"blame", line:12, before:true} });
      window.party.client.shareCue();
    });
    await expect(guest.page.getByRole("button", { name: "host: src/lib.rs:12" })).toBeVisible();
    await expect(guest.page.getByRole("button", { name: "Fly to view" })).toBeEnabled();

    for (const { page } of [host, guest]) await page.getByRole("button", { name: "Enable voice" }).click();
    await host.page.evaluate(() => window.party.client.talk(true));
    await expect.poll(() => guest.page.evaluate(async () => {
      const stats = await Promise.all(window.party.peers.map(peer => peer.getStats()));
      return stats.flatMap(report => [...report.values()]).some(item =>
        item.type === "inbound-rtp" && item.kind === "audio" && item.bytesReceived > 0);
    }), { timeout: 20_000 }).toBe(true);
    await host.page.evaluate(() => window.party.client.talk(false));

    if (process.env.REPO_CITY_TEST_RELAY === "1") {
      expect(await guest.page.evaluate(async () => {
        const stats = await Promise.all(window.party.peers.map(peer => peer.getStats()));
        return stats.some(report => [...report.values()].some(transport => {
          const pair = report.get(transport.selectedCandidatePairId);
          return pair && report.get(pair.localCandidateId)?.candidateType === "relay"
            && report.get(pair.remoteCandidateId)?.candidateType === "relay";
        }));
      })).toBe(true);
    }

    await host.page.evaluate(() => window.party.sockets.at(-1).close());
    await expect.poll(() => host.page.evaluate(() => window.party.sockets.length)).toBe(2);
    await expect(host.page.getByRole("status").first()).toHaveText("Connected");
    await expect(guest.page.getByRole("button", { name: "host: src/lib.rs:12" })).toHaveCount(1);

    await host.page.getByRole("button", { name: "Remove guest" }).click();
    await expect(guest.page.getByRole("status").first()).toContainText("Membership ended");
    expect(await guest.page.evaluate(() => window.party.captureStopped())).toBe(true);
    await host.page.getByRole("button", { name: "Close room" }).click();
    await expect(host.page.getByRole("status").first()).toContainText("Room ended");
  } finally {
    await Promise.all([host.context.close(), guest.context.close()]);
  }
});
