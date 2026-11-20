import test from "node:test";
import assert from "node:assert/strict";
import { createReviewParty } from "./review-party-client.mjs";

const comparison = { owner: "a", repo: "b", head_sha: "a".repeat(40), base_sha: "b".repeat(40), layout_version: 1 };
function fixture(extra = {}) {
  const requests = [], jumps = [], sockets = [];
  class Socket {
    readyState = 1; sent = [];
    constructor() { sockets.push(this); }
    send(value) { this.sent.push(JSON.parse(value)); }
    close() { this.readyState = 3; }
    receive(data) { this.onmessage({ data: JSON.stringify(data) }); }
  }
  const result = { room: { id: "room", host_id: "me", comparison }, member: { id: "me", login: "alice" }, socket_url: "/party/rooms/room/socket", ice_servers: [{ urls: "turn:relay", username: "u", credential: "c" }] };
  const client = createReviewParty({ fetch: async (url, options) => {
    requests.push({ url, body: JSON.parse(options?.body ?? "{}") });
    return { ok: true, json: async () => result };
  }, WebSocket: Socket, origin: "https://city.test", loadComparison: async value => { assert.deepEqual(value, comparison); }, navigate: value => jumps.push(value), setTimeout: () => 1, clearTimeout() {}, ...extra });
  return { client, requests, jumps, sockets };
}

test("Given a pinned comparison, creating a room never requests the microphone and follows only explicit selection", async () => {
  let captures = 0;
  const { client, requests, sockets, jumps } = fixture({ getUserMedia: async () => { captures++; } });
  await client.create(comparison);
  assert.equal(requests[0].url, "/party/rooms");
  sockets[0].receive({ type: "roster", members: [{ id: "me", login: "alice" }, { id: "peer", login: "bob", focus: { path: "src/main.rs", face: "blame", side: "before", line: 9 } }] });
  assert.equal(captures, 0);
  assert.deepEqual(jumps, []);
  client.follow("peer");
  assert.deepEqual(jumps, [{ path: "src/main.rs", face: "blame", before: true, line: 9 }]);
  client.dispose();
});

test("Given server ICE configuration, peer voice uses TURN and agent input requires both connection and each speaker's consent", async () => {
  const peers = [], attached = [], detached = [];
  let agentConnected = true;
  class Stream { constructor(tracks) { this.tracks = tracks; } getTracks() { return this.tracks; } getAudioTracks() { return this.tracks; } }
  const track = { stop() {}, clone() { return { stop() {} }; } };
  class Peer {
    constructor(config) { this.config = config; peers.push(this); }
    addTrack() {} close() { this.closed = true; }
    async createOffer() { return { type: "offer", sdp: "offer-sdp" }; }
    async setLocalDescription(description) { this.localDescription = description; }
  }
  const { client, sockets } = fixture({ RTCPeerConnection: Peer, MediaStream: Stream, getUserMedia: async () => new Stream([track]),
    playRemote: () => ({ play: async () => {}, remove() {} }),
    agentAdapter: { connected: () => agentConnected, attach: ({ participant }) => { attached.push(participant); return () => detached.push(participant); } },
  });
  await client.create(comparison);
  sockets[0].receive({ type: "roster", members: [{ id: "me", login: "alice" }, { id: "peer", login: "bob", voice: { enabled: true } }] });
  await client.enableVoice();
  assert.deepEqual(peers[0].config.iceServers, [{ urls: "turn:relay", username: "u", credential: "c" }]);
  client.setAgentInput(true);
  assert.deepEqual(attached, []);
  client.setShareWithAgents(true);
  assert.deepEqual(attached, ["me"]);
  peers[0].ontrack({ streams: [new Stream([track])], track });
  assert.ok(!attached.includes("peer"));
  sockets[0].receive({ type: "voice", from: "peer", enabled: true, share_with_agents: true });
  assert.ok(attached.includes("peer"));
  agentConnected = false;
  client.update({});
  assert.equal(client.state().agentInput, false);
  assert.ok(detached.includes("peer"));
  assert.equal(client.state().voice, true);
  client.dispose();
});

test("Given a pose burst, focus and cues stay independent and denied membership ends media", async () => {
  const { client, sockets } = fixture();
  await client.create(comparison); client.setLiveFocus(true);
  const pose = { position: [1, 2, 3], forward: [0, 0, -1] };
  for (let i = 0; i < 100; i++) client.update({ pose, focus: { path: "a.rs", before: false, face: "source", line: 1 } }, 1000 + i);
  assert.equal(sockets[0].sent.filter(value => value.type === "pose").length, 1);
  client.update({ focus: { path: "b.rs", before: true, face: "diff", line: 3 } }, 1100);
  client.shareCue();
  assert.equal(sockets[0].sent.at(-1).cue.path, "b.rs");
  sockets[0].onclose({ code: 4003, reason: "Removed" });
  assert.equal(client.state().room, undefined);
  assert.equal(client.state().status, "removed");
});

test("Given rejected access, joining leaves no socket or microphone active", async () => {
  const { client, sockets } = fixture({ fetch: async () => ({ ok: false, status: 403, json: async () => ({ error: "Invitation belongs to another account" }) }) });
  await assert.rejects(client.join("room", "invite"), /another account/);
  assert.equal(sockets.length, 0);
  assert.equal(client.state().status, "error");
});

test("Given permission denial, voice fails while membership and navigation remain usable", async () => {
  const { client } = fixture({ getUserMedia: async () => { throw new Error("Permission denied"); } });
  await client.create(comparison);
  await assert.rejects(client.enableVoice(), /Permission denied/);
  assert.equal(client.state().status, "connected");
  assert.equal(client.state().voice, false);
  client.dispose();
});

test("Given local capture, push-to-talk gates only party track and focus loss stops capture", async () => {
  const partyTrack = { enabled: true, stop() { this.stopped = true; } };
  const micTrack = { stop() { this.stopped = true; }, clone() { return partyTrack; } };
  const stream = { getAudioTracks: () => [micTrack], getTracks: () => [micTrack] };
  const { client } = fixture({ getUserMedia: async () => stream, MediaStream: class { constructor(tracks) { this.tracks = tracks; } getTracks() { return this.tracks; } } });
  await client.create(comparison);
  await client.enableVoice();
  assert.equal(partyTrack.enabled, false);
  client.talk(true); assert.equal(partyTrack.enabled, true);
  client.suspend();
  assert.equal(micTrack.stopped, true);
  assert.equal(partyTrack.stopped, true);
  assert.equal(client.state().voice, false);
  assert.equal(client.state().status, "connected");
  client.dispose();
});
