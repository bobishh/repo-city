import assert from "node:assert/strict";
import { test } from "node:test";
import { createCityAudio, spokenWords, chooseLocalVoice } from "./audio.mjs";
import { CitySynth } from "./audio-worklet.mjs";

function browser(storage = new Map()) {
  const document = new EventTarget();
  document.visibilityState = "visible";
  const host = new EventTarget();
  return Object.assign(host, { document, performance, localStorage: {
    getItem: key => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
  } });
}

test("audio preferences persist mute and disabled ambience without starting an unsupported engine", () => {
  const storage = new Map();
  const first = createCityAudio({ browser: browser(storage) });
  assert.equal(first.available().supported, false);
  assert.equal(first.available().state, "unsupported");
  first.updateSettings({ ambience: false, speech: false, volume: 0 });
  const second = createCityAudio({ browser: browser(storage) });
  assert.deepEqual(second.settings(), { ambience: false, speech: false, volume: 0 });
  assert.throws(() => first.updateSettings({ volume: Infinity }));
  assert.throws(() => first.updateSettings({ ambience: "yes" }));
  const copy = first.settings();
  copy.volume = 1;
  assert.equal(first.settings().volume, 0);
  first.dispose();
  second.dispose();
});

test("unavailable preference storage leaves usable session settings", () => {
  const host = browser();
  host.localStorage = { getItem() { throw new Error("disabled"); }, setItem() { throw new Error("disabled"); } };
  const audio = createCityAudio({ browser: host });
  audio.updateSettings({ volume: 0.4 });
  assert.equal(audio.settings().volume, 0.4);
  audio.dispose();
});

test("worklet synth produces bounded deterministic stereo signal and fades to silence", () => {
  const a = new CitySynth(123), b = new CitySynth(123);
  const left = new Float32Array(128), right = new Float32Array(128);
  const otherLeft = new Float32Array(128), otherRight = new Float32Array(128);
  const levels = { wind: 1, spray: 1, works: 1, siren: 1, pan: 0.6 };
  let energy = 0;
  for (let block = 0; block < 375; block++) {
    a.render(left, right, levels, 48000);
    b.render(otherLeft, otherRight, levels, 48000);
    assert.deepEqual(left, otherLeft);
    assert.deepEqual(right, otherRight);
    for (const sample of left) { assert.ok(Number.isFinite(sample) && Math.abs(sample) <= 1); energy += Math.abs(sample); }
  }
  assert.ok(energy > 1);
  for (let block = 0; block < 375; block++) a.render(left, right, {}, 48000);
  assert.ok(left.every(sample => Math.abs(sample) < 0.00001));
});


test("code speech uses clear local voices and separates identifiers instead of reading punctuation", () => {
  const novelty = { name: "Zarvox", lang: "en-US", localService: true };
  const remote = { name: "Samantha Online", lang: "en-US", localService: false };
  const natural = { name: "Samantha", lang: "en-US", localService: true };
  assert.equal(chooseLocalVoice([novelty, remote, natural]), natural);
  assert.equal(chooseLocalVoice([novelty, remote]), undefined);
  assert.deepEqual(spokenWords("veryLongFunctionName(good_input) HTTPResponse λογος"), ["very", "Long", "Function", "Name", "good", "input", "HTTP", "Response", "λογος"]);
});

test("browser synthesis keeps high-frequency sample changes within a quiet transient budget", () => {
  const synth = new CitySynth(123);
  const left = new Float32Array(128), right = new Float32Array(128);
  let previous = 0, largest = 0, energy = 0, samples = 0;
  for (let block = 0; block < 750; block++) {
    synth.render(left, right, { wind: 1, spray: 1, works: 1, siren: 1, pan: 0.6 }, 48000);
    for (const sample of right) {
      const difference = 0.4 * sample - previous;
      previous = 0.4 * sample;
      largest = Math.max(largest, Math.abs(difference)); energy += difference ** 2; samples++;
    }
  }
  const rms = Math.sqrt(energy / samples);
  assert.ok(largest < 0.008 && rms < 0.0015, `sample step ${largest}, difference RMS ${rms}`);
});

test("leaving a readable block finishes its phrase; mute still cancels it immediately", () => {
  const host = browser();
  let clock = 0, cancelled = 0;
  const spoken = [];
  host.performance = { now: () => clock };
  host.SpeechSynthesisUtterance = class { constructor(text) { this.text = text; } };
  host.speechSynthesis = {
    speaking: false, pending: false,
    getVoices: () => [{ name: "Samantha", lang: "en-US", localService: true }],
    speak(utterance) { spoken.push(utterance); this.speaking = true; },
    cancel() { cancelled++; this.speaking = false; },
  };
  const audio = createCityAudio({ browser: host });
  const gesture = new Event("pointerdown");
  Object.defineProperty(gesture, "isTrusted", { value: true });
  host.document.dispatchEvent(gesture);
  const first = { id: "first", text: "read this code and finish the current phrase", seconds: 10 };
  const second = { id: "second", text: "then read a different block of source code", seconds: 10 };
  audio.activity({ focus: first });
  clock = 4000;
  audio.activity({ focus: first });
  assert.equal(spoken.length, 1);
  const previousCancellations = cancelled;
  audio.activity({ focus: null });
  assert.equal(cancelled, previousCancellations, "focus loss does not cancel the current phrase");
  audio.activity({ focus: second });
  clock = 8000;
  audio.activity({ focus: second });
  assert.equal(spoken.length, 1, "a busy voice does not queue another phrase");
  assert.equal(cancelled, previousCancellations);
  host.speechSynthesis.speaking = false;
  spoken[0].onend?.();
  audio.activity({ focus: second });
  assert.equal(spoken.length, 2);
  audio.activity({ focus: null });
  audio.updateSettings({ volume: 0 });
  assert.equal(cancelled, previousCancellations + 1, "mute cancels even after reading focus is gone");
  clock = 30000;
  audio.activity({ focus: null });
  assert.equal(spoken.length, 2);
  audio.dispose();
});
