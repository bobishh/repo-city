// Procedural city sounds run on the audio rendering thread, independently of the UI.
import { RadioSynth } from "./radio.mjs";
export class CitySynth {
  constructor(seed = 0xc17a) {
    this.random = seed || 1;
    this.frame = this.low = this.air = this.breeze = this.impact = this.impactTarget = this.thudPhase = this.phase = 0;
    this.wind = this.spray = this.works = this.siren = this.pan = 0;
    this.radio = new RadioSynth(seed);
    this.music = 0;
  }
  render(left, right, levels = {}, rate = 48000) {
    const smooth = 1 - Math.exp(-1 / (rate * 0.05));
    const wind = levels.wind ?? 0, spray = levels.spray ?? 0, works = levels.works ?? 0;
    const siren = levels.siren ?? 0, pan = levels.pan ?? 0;
    const music = levels.music ?? 0;
    const station = levels.radio === 1 || levels.radio === 2 ? levels.radio : 0;
    const constructionCycle = Math.round(rate * 31), impactCycle = Math.round(rate * 0.49);
    for (let index = 0; index < left.length; index++) {
      this.wind += (wind - this.wind) * smooth;
      this.spray += (spray - this.spray) * smooth;
      this.works += (works - this.works) * smooth;
      this.siren += (siren - this.siren) * smooth;
      this.pan += (pan - this.pan) * smooth;
      this.music += (music - this.music) * smooth;
      this.radio.tick(station, rate);
      this.random ^= this.random << 13; this.random ^= this.random >>> 17; this.random ^= this.random << 5;
      const white = (this.random >>> 0) / 4294967295 * 2 - 1;
      this.low += (white - this.low) * 0.008;
      this.air += (white - this.air) * 0.06;
      this.breeze += (this.low - this.breeze) * 0.04;
      const construction = this.frame % constructionCycle;
      if (construction < rate * 2 && construction % impactCycle === 0) this.impactTarget = 1;
      this.impactTarget *= 0.9995;
      this.impact += (this.impactTarget - this.impact) * 0.001;
      this.thudPhase = (this.thudPhase + 73 / rate) % 1;
      const sirenTime = this.frame % Math.round(rate * 97) / rate;
      this.phase = (this.phase + (510 + 110 * Math.sin(sirenTime * 1.3)) / rate) % 1;
      const envelope = sirenTime < 5 ? Math.max(0, Math.sin(Math.PI * sirenTime / 5)) ** 2 : 0;
      const mixed = this.breeze * 0.9 * this.wind + (this.air - this.low) * 0.085 * this.spray +
        this.impact * (Math.sin(Math.PI * 2 * this.thudPhase) * 0.045 + this.air * 0.025) * this.works +
        Math.sin(Math.PI * 2 * this.phase) * envelope * 0.015 * this.siren;
      left[index] = Math.max(-1, Math.min(1, mixed * (1 - this.pan * 0.4) + this.radio.left * this.music));
      right[index] = Math.max(-1, Math.min(1, mixed * (1 + this.pan * 0.4) + this.radio.right * this.music));
      this.frame++;
    }
  }
}

if (typeof registerProcessor === "function") {
  class CityProcessor extends AudioWorkletProcessor {
    constructor() {
      super();
      this.synth = new CitySynth(Math.floor(Math.random() * 0x1_0000_0000));
      this.levels = { wind: 0.2 };
      this.lastActivity = 0;
      this.frames = 0;
      this.port.onmessage = ({ data }) => { this.levels = data; this.lastActivity = this.frames; };
    }
    process(_inputs, outputs) {
      const channels = outputs[0];
      if (channels.length < 2) return true;
      if (this.frames - this.lastActivity > sampleRate) this.levels.spray = 0;
      this.synth.render(channels[0], channels[1], this.levels, sampleRate);
      this.frames += channels[0].length;
      return true;
    }
  }
  registerProcessor("repo-city-atmosphere", CityProcessor);
}
