// Room events carry explicit chat, locations, and media signaling.
export function createReviewParty(options = {}) {
  const request = options.fetch ?? globalThis.fetch.bind(globalThis);
  const Socket = options.WebSocket ?? globalThis.WebSocket;
  const Peer = options.RTCPeerConnection ?? globalThis.RTCPeerConnection;
  const Stream = options.MediaStream ?? globalThis.MediaStream;
  const capture = options.getUserMedia ?? (constraints => navigator.mediaDevices.getUserMedia(constraints));
  const later = options.setTimeout ?? globalThis.setTimeout;
  const cancel = options.clearTimeout ?? globalThis.clearTimeout;
  const origin = options.origin ?? globalThis.location.origin;
  const listeners = new Set(), peers = new Map(), remoteStreams = new Map(), routes = new Map(), muted = new Set();
  let room, member, socket, credentials, renewal, reconnect, retry = 0, generation = 0;
  let status = "solo", error = "", members = [], cues = [], chat = [], sketch = null, sequence = 0;
  let microphone, outgoing, speaking = false, liveFocus = false, agentInput = false, shareWithAgents = false;
  let currentFocus = null, lastFocus = "", lastPose = 0, agentAdapter = options.agentAdapter;
  const connectedAgent = () => Boolean(agentAdapter?.connected());
  const state = () => ({ room, member, members, cues, chat, sketch, status, error, voice: Boolean(microphone), speaking,
    liveFocus, agentInput, shareWithAgents, agentConnected: connectedAgent(), muted: [...muted] });
  const changed = () => { for (const listener of listeners) listener(state()); };
  const send = value => { if (socket?.readyState === 1) socket.send(JSON.stringify(value)); };
  async function api(path, body = {}, token) {
    const headers = { "Content-Type": "application/json" };
    if (token) headers.Authorization = `Bearer ${token}`;
    const response = await request(`/party/${path}`, { method: "POST", credentials: "same-origin",
      headers, body: JSON.stringify(body) });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error ?? `Room request failed (${response.status}).`);
    return data;
  }
  const roomApi = (path, body) => api(`rooms/${room.id}/${path}`, body, path === "renew" ? undefined : credentials.room_token);
  function voiceState() {
    send({ type: "voice", enabled: Boolean(microphone), speaking, agent_input: agentInput && connectedAgent(), share_with_agents: shareWithAgents });
  }
  function clearRoutes() { for (const close of routes.values()) close?.(); routes.clear(); }
  function updateAgentRoutes() {
    clearRoutes();
    if (!agentInput || !connectedAgent()) {
      if (agentInput) { agentInput = false; voiceState(); changed(); }
      return;
    }
    if (microphone && shareWithAgents) routes.set(member.id, agentAdapter.attach({ participant: member.id, stream: microphone }));
    for (const [id, stream] of remoteStreams) {
      if (members.find(value => value.id === id)?.voice?.share_with_agents) {
        routes.set(id, agentAdapter.attach({ participant: id, stream }));
      }
    }
  }
  function dropPeer(id) {
    const entry = peers.get(id);
    if (entry) { entry.pc.close(); entry.audio?.remove?.(); peers.delete(id); }
    remoteStreams.delete(id);
  }
  function disableVoice() {
    for (const stream of [microphone, outgoing]) for (const track of stream?.getTracks() ?? []) track.stop();
    microphone = outgoing = undefined; speaking = false;
    for (const id of [...peers.keys()]) dropPeer(id);
    clearRoutes(); voiceState(); changed();
  }
  const mediaError = problem => { error = `Voice unavailable: ${problem.message ?? problem}`; changed(); };
  const iceConfiguration = () => ({
    iceServers: credentials.ice_servers ?? [], iceTransportPolicy: credentials.ice_transport_policy ?? "all",
  });
  function peer(id) {
    if (peers.has(id)) return peers.get(id);
    const pc = new Peer(iceConfiguration());
    const entry = { pc, pending: [], chain: Promise.resolve() };
    peers.set(id, entry);
    for (const track of outgoing?.getTracks() ?? []) pc.addTrack(track, outgoing);
    pc.onicecandidate = ({ candidate }) => { if (candidate) send({ type: "signal", to: id, signal: { type: "ice", candidate: candidate.toJSON?.() ?? candidate } }); };
    pc.ontrack = event => {
      const stream = event.streams[0] ?? new Stream([event.track]);
      remoteStreams.set(id, stream);
      entry.audio = options.playRemote ? options.playRemote(id, stream) : Object.assign(document.createElement("audio"), { srcObject: stream, autoplay: true });
      entry.audio.muted = muted.has(id);
      entry.audio.play?.().catch(() => { error = "Select Enable voice to allow remote audio playback."; changed(); });
      updateAgentRoutes();
    };
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === "failed") mediaError(new Error("Connection failed. Check the configured TURN relay or re-enable voice."));
    };
    return entry;
  }
  async function offer(id) {
    const { pc } = peer(id);
    await pc.setLocalDescription(await pc.createOffer());
    send({ type: "signal", to: id, signal: { type: "offer", sdp: pc.localDescription.sdp } });
  }
  function syncPeers() {
    const enabled = new Set(members.filter(value => value.id !== member?.id && value.voice?.enabled).map(value => value.id));
    for (const id of [...peers.keys()]) if (!enabled.has(id)) dropPeer(id);
    if (microphone) for (const id of enabled) {
      if (!peers.has(id) && String(member.id).localeCompare(String(id)) < 0) void offer(id).catch(mediaError);
    }
    updateAgentRoutes();
  }
  async function signal(from, signal) {
    if (!microphone || !members.some(value => value.id === from && value.voice?.enabled)) return;
    const entry = peer(from);
    entry.chain = entry.chain.then(async () => {
      const { pc } = entry;
      if (signal.type === "ice") {
        if (pc.remoteDescription) await pc.addIceCandidate(signal.candidate);
        else entry.pending.push(signal.candidate);
      } else {
        await pc.setRemoteDescription({ type: signal.type, sdp: signal.sdp });
        for (const candidate of entry.pending.splice(0)) await pc.addIceCandidate(candidate);
        if (signal.type === "offer") {
          await pc.setLocalDescription(await pc.createAnswer());
          send({ type: "signal", to: from, signal: { type: "answer", sdp: pc.localDescription.sdp } });
        }
      }
    }).catch(mediaError);
    await entry.chain;
  }
  function receiveChat(message) {
    if (!chat.some(item => item.seq === message.seq)) {
      chat = [...chat, message].sort((left, right) => left.seq - right.seq).slice(-32);
    }
  }
  function receive(message) {
    if (message.type === "roster") {
      members = message.members; room = message.room ?? room; syncPeers();
      if (!members.some(value => value.id === member.id)) { disconnect("removed", "Room membership ended."); return; }
    } else if (message.type === "focus" || message.type === "pose" || message.type === "voice") {
      const person = members.find(value => value.id === message.from);
      if (person) person[message.type] = message.type === "focus" ? message.focus : message.type === "pose" ? message : {
        enabled: message.enabled, speaking: message.speaking, agent_input: message.agent_input, share_with_agents: message.share_with_agents,
      };
      if (message.type === "voice") syncPeers();
    } else if (message.type === "cue") {
      if (message.seq == null || !cues.some(cue => cue.seq === message.seq)) {
        cues = [...cues.slice(-31), { ...message.cue, from: message.from, seq: message.seq }];
      }
    } else if (message.type === "chat") receiveChat(message);
    else if (message.type === "sketch" && (!sketch || message.seq > sketch.seq)) sketch = message;
    else if (message.type === "signal") { void signal(message.from, message.signal).catch(mediaError); return; }
    else if (["closed", "removed", "expired"].includes(message.type)) { disconnect(message.type, message.reason ?? "Room access ended."); return; }
    else if (message.type === "error") error = message.error ?? "Room request rejected.";
    changed();
  }
  function connect(epoch) {
    if (epoch !== generation || !room) return;
    const url = new URL(credentials.socket_url, origin);
    if (url.origin !== origin) throw new Error("Invalid room socket origin.");
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const connection = socket = credentials.room_token
      ? new Socket(url.href, ["repo-city-party", credentials.room_token])
      : new Socket(url.href);
    connection.onopen = () => { if (epoch !== generation) return; status = "connected"; retry = 0; error = ""; lastFocus = ""; voiceState(); changed(); };
    connection.onmessage = event => { if (epoch === generation) { try { receive(JSON.parse(event.data)); } catch { error = "Invalid room update."; changed(); } } };
    connection.onclose = event => {
      if (epoch !== generation) return;
      if (event?.code === 4003 || event?.code === 4000) { disconnect("removed", event.reason || "Room membership ended."); return; }
      disableVoice(); status = "reconnecting"; changed();
      reconnect = later(() => {
        void roomApi("renew").then(data => {
          if (epoch !== generation) return;
          credentials = { ...credentials, ...data }; connect(epoch);
        }).catch(problem => { if (epoch === generation) disconnect("error", problem.message); });
      }, Math.min(30_000, 1000 * 2 ** retry++));
    };
    connection.onerror = () => { error = "Room connection interrupted."; changed(); };
    // A socket may already be open in an embedded client or test transport.
    if (connection.readyState === 1) connection.onopen();
  }
  function renew(epoch) {
    renewal = later(async () => {
      if (epoch !== generation) return;
      try {
        const updated = await roomApi("renew");
        if (epoch !== generation) return;
        credentials = { ...credentials, ...updated };
        for (const { pc } of peers.values()) pc.setConfiguration?.(iceConfiguration());
        renew(epoch);
      } catch (problem) { if (epoch === generation) disconnect("error", problem.message); }
    }, 240_000);
  }
  function disconnect(next = "solo", message = "") {
    generation++; cancel(renewal); cancel(reconnect); disableVoice();
    socket?.close(); socket = undefined; room = member = credentials = undefined;
    members = []; cues = []; chat = []; sketch = null; status = next; error = message; liveFocus = agentInput = shareWithAgents = false; changed();
  }
  async function enter(path, body) {
    if (room) await leave();
    status = "connecting"; error = ""; changed();
    let data;
    try {
      data = await api(path, body);
      await options.loadComparison?.(data.room.comparison);
      credentials = data; room = data.room; member = data.member; members = data.members ?? [member];
      for (const message of data.events ?? []) {
        if (message.type === "chat" || message.type === "sketch") receive(message);
      }
      const epoch = ++generation; connect(epoch); renew(epoch); changed();
      return state();
    } catch (problem) {
      if (data?.room) await api(`rooms/${data.room.id}/leave`, {}, data.room_token).catch(() => {});
      disconnect("error", problem.message); throw problem;
    }
  }
  async function leave() {
    const id = room?.id, token = credentials?.room_token;
    disconnect();
    if (id) await api(`rooms/${id}/leave`, {}, token);
  }
  function navigate(anchor) {
    if (!anchor) return;
    options.navigate?.({ path: anchor.path, face: anchor.face ?? "source", before: anchor.side === "before" || anchor.before === true,
      ...(anchor.line ? { line: anchor.line } : {}), ...(anchor.end_line ? { end_line: anchor.end_line } : {}) });
  }
  return {
    state, subscribe(listener) { listeners.add(listener); listener(state()); return () => listeners.delete(listener); },
    create: comparison => enter("rooms", { comparison }), join: (id, invite) => enter(`rooms/${encodeURIComponent(id)}/join`, { invite }),
    leave, invite: login => roomApi("invites", { login }), revokeInvite: invite => roomApi("revoke-invite", { invite }),
    remove: member_id => roomApi("remove", { member_id }),
    async sendChat(id, text) {
      if (status !== "connected") throw new Error("Connect to the room to send a message.");
      await roomApi("events", { type: "chat", id, text });
    },
    async shareSketch(title, source, format) {
      if (status !== "connected") throw new Error("Connect to the room to share a sketch.");
      await roomApi("events", { type: "sketch", title, source, format });
    },
    async close() { await roomApi("close"); disconnect("closed", "Host closed the room."); },
    follow: id => navigate(members.find(value => value.id === id)?.focus), followCue: navigate,
    shareCue() { if (currentFocus) send({ type: "cue", cue: currentFocus }); },
    setLiveFocus(enabled) { liveFocus = enabled; lastFocus = ""; if (!enabled) send({ type: "focus", focus: null }); changed(); },
    update({ pose, focus }, now = Date.now()) {
      if (!room || status !== "connected") return;
      currentFocus = focus ? { ...focus, side: focus.before ? "before" : "after" } : null;
      if (liveFocus && JSON.stringify(currentFocus) !== lastFocus) { lastFocus = JSON.stringify(currentFocus); send({ type: "focus", focus: currentFocus }); }
      if (pose && now - lastPose >= 250 && (socket?.bufferedAmount ?? 0) < 16_384) { lastPose = now; send({ type: "pose", ...pose, seq: ++sequence }); }
      if (agentInput && !connectedAgent()) updateAgentRoutes();
    },
    async enableVoice() {
      if (!room || microphone) return;
      try {
        const epoch = generation, stream = await capture({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true }, video: false });
        if (epoch !== generation || !room) { stream.getTracks().forEach(track => track.stop()); return; }
        microphone = stream;
        outgoing = new Stream(stream.getAudioTracks().map(track => { const copy = track.clone(); copy.enabled = false; return copy; }));
        error = ""; voiceState(); syncPeers(); changed();
      } catch (problem) { mediaError(problem); throw problem; }
    },
    disableVoice, talk(enabled) { speaking = Boolean(enabled && outgoing); for (const track of outgoing?.getTracks() ?? []) track.enabled = speaking; voiceState(); changed(); },
    mute(id, enabled) { if (enabled) muted.add(id); else muted.delete(id); const audio = peers.get(id)?.audio; if (audio) audio.muted = enabled; changed(); },
    setShareWithAgents(enabled) { shareWithAgents = Boolean(enabled); voiceState(); updateAgentRoutes(); changed(); },
    setAgentInput(enabled) { agentInput = Boolean(enabled && connectedAgent()); voiceState(); updateAgentRoutes(); changed(); },
    setAgentAdapter(adapter) { agentAdapter = adapter; updateAgentRoutes(); changed(); },
    suspend() { disableVoice(); currentFocus = null; send({ type: "focus", focus: null }); },
    dispose() { disconnect(); listeners.clear(); },
  };
}
