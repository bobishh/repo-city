const preferenceKey = "repo-city-audio-v1";
const defaults = { ambience: true, speech: true, volume: 0.18 };
const clamp = (value, min, max) => Number.isFinite(value) ? Math.max(min, Math.min(max, value)) : min;

function validated(next, previous = defaults) {
  if (next && typeof next === "object" && !Array.isArray(next)) {
    const value = { ...previous };
    for (const key of ["ambience", "speech"]) {
      if (next[key] !== undefined) {
        if (typeof next[key] !== "boolean") throw new Error(`${key} must be on or off.`);
        value[key] = next[key];
      }
    }
    if (next.volume !== undefined) {
      if (!Number.isFinite(next.volume) || next.volume < 0 || next.volume > 1) throw new Error("Volume must be between 0 and 1.");
      value.volume = next.volume;
    }
    return value;
  }
  return { ...previous };
}

export function spokenWords(text) {
  return text.replace(/(\p{Lu})(\p{Lu}\p{Ll})/gu, "$1 $2")
    .replace(/(\p{Ll})(\p{Lu})/gu, "$1 $2")
    .replace(/[^\p{L}\p{N}\s]/gu, " ").trim().split(/\s+/u);
}

export function chooseLocalVoice(voices) {
  const local = voices.filter(voice => voice.localService === true);
  const clear = voice => !/Albert|Zarvox|Trinoids|Whisper|Bells|Boing|Bubbles|Cellos|Bad News|Good News|Hysterical|Organ/i.test(voice.name);
  return local.find(voice => /Samantha/i.test(voice.name)) ??
    local.find(voice => voice.default && /^en\b/i.test(voice.lang) && clear(voice)) ??
    local.find(voice => /^en\b/i.test(voice.lang) && clear(voice)) ?? local.find(voice => voice.default && clear(voice));
}

export function createCityAudio({ browser = globalThis, active = () => true, onOutput } = {}) {
  const document = browser.document;
  const Context = browser.AudioContext ?? browser.webkitAudioContext;
  const supported = typeof Context === "function" && typeof browser.AudioWorkletNode === "function";
  let preferences = { ...defaults };
  try { preferences = validated(JSON.parse(browser.localStorage?.getItem(preferenceKey) ?? "null")); } catch { /* Session defaults remain usable. */ }
  let context, gain, node, loading, suspendTimer, error = "", unlocked = false, disposed = false;
  let activity = {}, focus, currentUtterance, readingStarted = 0, nextQuote = 0;
  const speech = browser.speechSynthesis;
  const now = () => browser.performance?.now() ?? Date.now();
  const visible = () => document?.visibilityState !== "hidden";
  const playing = () => { try { return visible() && Boolean(active()); } catch { return false; } };
  const localVoice = () => {
    try { return chooseLocalVoice(speech?.getVoices() ?? []); } catch { return undefined; }
  };

  function cancelSpeech() {
    if (currentUtterance) { try { speech?.cancel(); } catch { /* Speech may be unavailable. */ } }
    currentUtterance = undefined;
    focus = undefined;
  }

  function refresh() {
    if (disposed) return;
    const audible = playing() && preferences.ambience && preferences.volume > 0;
    if (gain) {
      gain.gain.cancelScheduledValues(context.currentTime);
      gain.gain.setTargetAtTime(audible ? preferences.volume : 0, context.currentTime, 0.025);
    }
    if (node) node.port.postMessage({
      wind: clamp(activity.wind ?? 0.2, 0, 1), spray: activity.blocked ? 0 : clamp(activity.spray ?? 0, 0, 1),
      works: activity.changed ? 0.8 : 0.12, siren: 0.5, pan: clamp(activity.pan ?? 0, -1, 1),
    });
    // Suspending an inaudible graph avoids background synthesis and autoplay surprises.
    if (audible && suspendTimer) { browser.clearTimeout(suspendTimer); suspendTimer = undefined; }
    if (context?.state === "running" && !audible && !suspendTimer) suspendTimer = browser.setTimeout(() => {
      suspendTimer = undefined;
      if (!playing() || !preferences.ambience || preferences.volume === 0) context.suspend().catch(() => {});
    }, 240);
    else if (context?.state === "suspended" && audible && unlocked) context.resume().catch(() => {});
    if (!playing() || activity.silenceSpeech || !preferences.speech || preferences.volume === 0) cancelSpeech();
  }

  function start() {
    if (!supported || disposed || !preferences.ambience || preferences.volume === 0) return;
    try {
      context ??= new Context();
      // Resume synchronously inside the trusted gesture, before loading the worklet.
      context.resume().catch(() => {});
      if (!loading) loading = context.audioWorklet.addModule(new URL("./audio-worklet.mjs", import.meta.url)).then(() => {
        if (disposed) return;
        node = new browser.AudioWorkletNode(context, "repo-city-atmosphere", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2] });
        gain = context.createGain();
        gain.gain.value = 0;
        node.connect(gain); gain.connect(context.destination);
        onOutput?.(context, gain);
        node.onprocessorerror = () => { error = "City sound processing stopped."; gain.gain.value = 0; };
        refresh();
      }).catch(failure => { error = failure.message || "City audio is unavailable."; context?.suspend().catch(() => {}); });
    } catch (failure) { error = failure.message || "City audio is unavailable."; }
  }

  function gesture(event) {
    if (!event.isTrusted || event.repeat) return;
    unlocked = true;
    start();
  }

  function quote() {
    if (!unlocked || !playing() || activity.silenceSpeech || !preferences.speech || preferences.volume === 0) { cancelSpeech(); return; }
    const next = activity.focus;
    if (activity.blocked || !localVoice() || !next?.id || typeof next.text !== "string" || !Number.isFinite(next.seconds) || next.seconds <= 0) { focus = undefined; return; }
    if (focus?.id !== next.id) {
      focus = next;
      readingStarted = now();
      nextQuote = readingStarted + Math.min(4.5, Math.max(1.5, next.seconds * 0.3)) * 1000;
    }
    const elapsed = (now() - readingStarted) / 1000;
    if (now() < nextQuote || elapsed > next.seconds || speech.speaking || speech.pending) return;
    nextQuote = now() + (9 + Math.random() * 8) * 1000;
    const words = spokenWords(next.text);
    if (!words[0]) return;
    const position = Math.min(words.length - 1, Math.floor(elapsed / next.seconds * words.length));
    const text = words.slice(position, position + 3 + Math.floor(Math.random() * 4)).join(" ");
    try {
      const utterance = new browser.SpeechSynthesisUtterance(text);
      utterance.voice = localVoice();
      utterance.volume = preferences.volume;
      utterance.rate = 0.95;
      utterance.pitch = 1;
      currentUtterance = utterance;
      utterance.onend = utterance.onerror = () => { if (currentUtterance === utterance) currentUtterance = undefined; };
      speech.speak(utterance);
    } catch { currentUtterance = undefined; /* Ambient sound continues if the local speech engine fails. */ }
  }

  const visibility = () => { refresh(); quote(); };
  const unload = () => api.dispose();
  document?.addEventListener("pointerdown", gesture, true);
  document?.addEventListener("keydown", gesture, true);
  document?.addEventListener("visibilitychange", visibility);
  browser.addEventListener?.("pagehide", unload);
  const api = {
    settings: () => ({ ...preferences }),
    updateSettings(next) {
      preferences = validated(next, preferences);
      try { browser.localStorage?.setItem(preferenceKey, JSON.stringify(preferences)); } catch { /* Keep session preferences. */ }
      if (unlocked && preferences.ambience && preferences.volume > 0) start();
      refresh();
      return api.settings();
    },
    available: () => ({ supported, state: !supported ? "unsupported" : error ? "unavailable" : !context ? "locked" : !node ? "loading" : context.state, error, speech: Boolean(localVoice()) }),
    refresh,
    activity(value) {
      try { activity = typeof value === "string" ? JSON.parse(value) : value ?? {}; } catch { activity = {}; }
      refresh(); quote();
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      cancelSpeech();
      document?.removeEventListener("pointerdown", gesture, true);
      document?.removeEventListener("keydown", gesture, true);
      document?.removeEventListener("visibilitychange", visibility);
      browser.removeEventListener?.("pagehide", unload);
      if (suspendTimer) browser.clearTimeout(suspendTimer);
      node?.disconnect(); gain?.disconnect();
      context?.close().catch(() => {});
    },
  };
  return api;
}
