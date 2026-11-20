const element = (tag, text, attributes = {}) => {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  for (const [name, value] of Object.entries(attributes)) node.setAttribute(name, value);
  return node;
};

function inviteToken(url) {
  return new URLSearchParams(url.hash.slice(1)).get("invite") ?? url.searchParams.get("invite");
}

// This is a service projection; the existing VibePad adapter renders its pixels.
export function mountReviewParty(root, client, { comparison = () => null, clipboard = text => navigator.clipboard.writeText(text) } = {}) {
  const status = element("p", "Solo", { role: "status" });
  const pinned = element("p", "Open a repository comparison to create a room.");
  const controls = element("div", undefined, { class: "dialog-actions" });
  const inviteControls = element("div");
  const roster = element("section", undefined, { "aria-label": "Party members" });
  const cues = element("section", undefined, { "aria-label": "Shared locations" });
  const media = element("fieldset"); media.append(element("legend", "Voice"));
  let feedback = "", busy = false, membersKey = "", cuesKey = "", chatKey = "", roomId, pendingMessage;
  const act = action => async () => {
    if (busy) return;
    busy = true; feedback = ""; render(client.state());
    try { await action(); } catch (problem) { feedback = problem.message; }
    finally { busy = false; render(client.state()); }
  };
  function button(text, action, parent = controls) {
    const node = element("button", text, { type: "button" }); node.addEventListener("click", act(action)); parent.append(node); return node;
  }
  function input(label, parent) {
    const wrapper = element("label", `${label} `), node = element("input", undefined, { "aria-label": label });
    wrapper.append(node); parent.append(wrapper); return node;
  }
  function check(label, action) {
    const node = input(label, media); node.type = "checkbox"; node.addEventListener("change", () => action(node.checked)); return node;
  }
  const create = button("Create room", () => client.create(comparison()));
  const invitation = input("Invitation link", controls); invitation.type = "url";
  if (new URL(location.href).searchParams.has("party")) invitation.value = location.href;
  const join = button("Join room", () => {
    const url = new URL(invitation.value, location.origin);
    const token = inviteToken(url), id = url.searchParams.get("party");
    if (!id || !token) throw new Error("Paste a complete room invitation link.");
    return client.join(id, token);
  });
  const leave = button("Leave room", () => client.leave());
  const close = button("Close room", () => client.close());
  const chat = element("section", undefined, { "aria-label": "Room chat" });
  chat.append(element("h3", "Room chat"));
  const draft = element("textarea", undefined, { "aria-label": "Room message", maxlength: "4096", rows: "3" });
  chat.append(draft);
  const send = button("Send message", async () => {
    const text = draft.value.trim();
    if (pendingMessage?.text !== text) pendingMessage = { id: crypto.randomUUID(), text };
    const message = pendingMessage;
    await client.sendChat(message.id, text);
    if (draft.value.trim() === message.text) draft.value = "";
    pendingMessage = undefined;
  }, chat);
  const history = element("div", undefined, { "aria-live": "polite" });
  draft.addEventListener("input", () => render(client.state()));
  chat.append(history);
  const sketches = element("section", undefined, { "aria-label": "Room sketches" });
  const sketchStatus = element("p", "Waiting for a room sketch.");
  const publish = element("div");
  const title = input("Sketch title", publish); title.maxLength = 240;
  const format = element("select", undefined, { "aria-label": "Sketch format" });
  format.append(element("option", "Markdown", { value: "markdown" }), element("option", "Mermaid", { value: "mermaid" }));
  const source = element("textarea", undefined, { "aria-label": "Sketch source", maxlength: "16384", rows: "4" });
  publish.append(format, source);
  const shareSketch = button("Share sketch", () => client.shareSketch(title.value.trim(), source.value.trim(), format.value), publish);
  for (const field of [title, source]) field.addEventListener("input", () => render(client.state()));
  sketches.append(element("h3", "Sketches"), sketchStatus, publish);
  const login = input("Invite GitHub account", inviteControls); login.autocomplete = "off";
  const invite = button("Create invitation", async () => {
    const result = await client.invite(login.value.trim());
    inviteLink.value = new URL(result.invite_url, location.origin).href;
    feedback = "Invitation ready. Only that GitHub account can join.";
  }, inviteControls);
  const inviteLink = input("Share invitation", inviteControls); inviteLink.readOnly = true;
  const copy = button("Copy invitation", async () => { await clipboard(inviteLink.value); feedback = "Invitation copied."; }, inviteControls);
  const revoke = button("Revoke invitation", async () => {
    await client.revokeInvite(inviteToken(new URL(inviteLink.value, location.origin)));
    inviteLink.value = ""; feedback = "Invitation revoked.";
  }, inviteControls);
  const focus = check("Share live reading focus", enabled => client.setLiveFocus(enabled));
  const cue = button("Share current lines", () => client.shareCue(), media);
  const voice = button("Enable voice", () => client.enableVoice(), media);
  const stop = button("Disable voice", () => client.disableVoice(), media);
  const talk = element("p", "Microphone muted.", { role: "status" }); media.append(talk);
  media.append(element("p", "Hold F8 while flying to talk. Release to mute. Leaving the app stops microphone capture."));
  const consent = check("Allow my voice to reach party agents", enabled => client.setShareWithAgents(enabled));
  const agent = check("Send consenting participants to my connected agent", enabled => client.setAgentInput(enabled));
  const agentStatus = element("p", "No connected agent adapter. Human voice remains available."); media.append(agentStatus);
  root.append(status, pinned, controls, chat, inviteControls, sketches, media, roster, cues);

  function render(state) {
    const inRoom = Boolean(state.room), host = state.room?.host_id === state.member?.id;
    if (roomId !== state.room?.id) {
      roomId = state.room?.id;
      inviteLink.value = "";
      draft.value = "";
      title.value = source.value = "";
      pendingMessage = undefined;
    }
    const current = state.room?.comparison ?? comparison();
    const label = state.status === "connected" ? "Connected" : state.status === "connecting" ? "Connecting…" : state.status === "reconnecting" ? "Reconnecting…" : state.status === "solo" ? "Solo" : "Room ended";
    status.textContent = `${label}${feedback || state.error ? ` · ${feedback || state.error}` : ""}`;
    pinned.textContent = current ? `${current.owner}/${current.repo} · ${current.head_sha.slice(0, 8)}${current.base_sha ? ` against ${current.base_sha.slice(0, 8)}` : ""}` : "Open a repository comparison to create a room.";
    create.hidden = invitation.closest("label").hidden = join.hidden = inRoom;
    create.disabled = busy || !current;
    join.disabled = busy;
    leave.hidden = !inRoom; leave.disabled = busy;
    close.hidden = !inRoom || !host; close.disabled = busy;
    chat.hidden = !inRoom;
    sketches.hidden = !inRoom;
    publish.hidden = !host;
    shareSketch.disabled = busy || state.status !== "connected" || !title.value.trim() || !source.value.trim();
    sketchStatus.textContent = state.sketch?.title ?? "Waiting for a room sketch.";
    send.disabled = busy || state.status !== "connected" || !draft.value.trim();
    const nextChat = state.chat.map(message => message.seq).join(",");
    if (nextChat !== chatKey) {
      chatKey = nextChat;
      history.replaceChildren(...state.chat.slice().reverse().map(message => {
        const row = element("div");
        row.append(element("p", message.login), element("p", message.text));
        return row;
      }));
    }
    inviteControls.hidden = !inRoom || !host;
    invite.disabled = busy; copy.disabled = revoke.disabled = busy || !inviteLink.value;
    media.hidden = !inRoom;
    focus.checked = state.liveFocus; consent.checked = state.shareWithAgents; agent.checked = state.agentInput;
    voice.disabled = busy || state.voice; stop.disabled = !state.voice;
    talk.textContent = state.speaking ? "Transmitting to party while F8 is held." : "Microphone muted. Hold F8 while flying to talk.";
    agent.disabled = !state.agentConnected;
    agentStatus.textContent = state.agentConnected ? "Agent input includes only participants who allow sharing. Agent speech stays local." : "No connected agent adapter. Human voice remains available.";
    const nextMembers = JSON.stringify([state.members.map(({ id, login, focus, voice }) => ({ id, login, focus, voice })), state.muted]);
    if (nextMembers !== membersKey) {
      membersKey = nextMembers; roster.replaceChildren(element("h3", "Members"));
      for (const person of state.members) {
        const row = element("div"), anchor = person.focus;
        const details = [person.login, person.voice?.speaking ? "speaking" : "", person.voice?.agent_input ? "forwarding consenting voices to agent" : "",
          anchor ? `${anchor.path}${anchor.line ? `:${anchor.line}` : ""} · ${anchor.face} · ${anchor.side}` : "not sharing focus"].filter(Boolean).join(" · ");
        row.append(element("p", details));
        if (person.id !== state.member?.id) {
          const follow = button("Fly to view", () => client.follow(person.id), row); follow.disabled = !anchor;
          const muted = input(`Mute ${person.login}`, row); muted.type = "checkbox";
          muted.checked = state.muted.includes(person.id);
          muted.addEventListener("change", () => client.mute(person.id, muted.checked));
          if (host) button(`Remove ${person.login}`, () => client.remove(person.id), row);
        }
        roster.append(row);
      }
    }
    const nextCues = JSON.stringify(state.cues);
    if (nextCues !== cuesKey) {
      cuesKey = nextCues; cues.replaceChildren(element("h3", "Shared locations"));
      for (const anchor of state.cues) button(`${state.members.find(person => person.id === anchor.from)?.login ?? "Member"}: ${anchor.path}:${anchor.line ?? 1}`, () => client.followCue(anchor), cues);
    }
    roster.hidden = cues.hidden = !inRoom;
  }
  const unsubscribe = client.subscribe(render);
  return { refresh: () => render(client.state()), dispose: unsubscribe };
}
