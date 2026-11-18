import score from "../assets/audio/radio-score.json" with { type: "json" };

// Musical data is shared with the native synthesizer. No worklet-frame allocations.
export class RadioSynth {
  constructor(seed = 0xc17a) {
    this.random = seed || 1;
    this.station = this.fade = this.time = this.position = this.low = this.shift = 0;
    this.lastStep = -1;
    this.kickPhase = this.bassPhase = this.arpPhase = this.arpFrequency = 0;
    this.phases = new Float64Array(4);
    this.frequencies = new Float64Array(4);
    this.echo = new Float32Array(192000);
    this.left = this.right = 0;
    this.rate = 0;
  }

  tick(target, rate) {
    if (rate !== this.rate) { this.rate = rate; this.smoothing = 1 - Math.exp(-1 / (rate * 0.025)); }
    const changing = target !== this.station;
    this.fade += ((changing || target === 0 ? 0 : 1) - this.fade) * this.smoothing;
    if (changing && this.fade < 0.0001) {
      this.station = target; this.time = 0; this.lastStep = -1;
    }
    if (!this.station) { this.left = this.right = 0; return; }
    const techno = this.station === 2;
    const beatSeconds = 60 / score.bpm[this.station - 1];
    const beats = this.time / beatSeconds, step = Math.floor(beats * 4), stepFraction = beats * 4 - step;
    const beatTime = (beats % 1) * beatSeconds;
    const stepTime = stepFraction * beatSeconds / 4;
    const chord = score.chords[Math.floor(step / 32) % score.chords.length];
    this.random ^= this.random << 13; this.random ^= this.random >>> 17; this.random ^= this.random << 5;
    const white = (this.random >>> 0) / 4294967295 * 2 - 1;
    this.low += (white - this.low) * 0.12;
    if (step !== this.lastStep) {
      if (step % 128 === 0) this.shift = (this.random >>> 8) % 16;
      for (let i = 0; i < 4; i++) this.frequencies[i] = 440 * 2 ** ((chord[i] + 12 - 69) / 12);
      this.targetArp = 440 * 2 ** ((chord[score.arpeggio[(step + this.shift) % 16]] + (techno ? 12 : 24) - 69) / 12);
      this.bassFrequency = 440 * 2 ** ((chord[0] - 12 - 69) / 12);
      if (step % 4 === 0) this.kickPhase = 0;
      this.lastStep = step;
    }
    this.kickPhase = (this.kickPhase + (48 + 110 * Math.exp(-beatTime * 35)) / rate) % 1;
    this.bassPhase = (this.bassPhase + this.bassFrequency / rate) % 1;
    this.arpFrequency += (this.targetArp - this.arpFrequency) * 0.002;
    this.arpPhase = (this.arpPhase + this.arpFrequency / rate) % 1;
    const kick = Math.sin(Math.PI * 2 * this.kickPhase) * Math.min(1, beatTime / 0.003) * Math.exp(-beatTime * 12) * (techno ? 0.27 : 0.22);
    const duck = 0.35 + 0.65 * (1 - Math.exp(-beatTime * 12));
    const pluck = Math.max(0, Math.sin(Math.PI * stepFraction)) * Math.exp(-stepTime * 8);
    const bassOn = techno ? step % 8 === 0 || step % 8 === 3 || step % 8 === 6 : step % 4 === 2;
    const bass = bassOn ? (Math.sin(Math.PI * 2 * this.bassPhase) + 0.12 * Math.sin(Math.PI * 4 * this.bassPhase)) * pluck * 0.13 : 0;
    const arp = (Math.sin(Math.PI * 2 * this.arpPhase) + 0.1 * Math.sin(Math.PI * 4 * this.arpPhase)) * pluck * (techno ? 0.045 : 0.085) * duck;
    let padLeft = 0, padRight = 0;
    for (let i = 0; i < 4; i++) {
      this.phases[i] = (this.phases[i] + this.frequencies[i] / rate) % 1;
      padLeft += Math.sin(Math.PI * 2 * this.phases[i]);
      padRight += Math.sin(Math.PI * 2 * this.phases[i] + 0.018 * Math.sin(this.time * 0.8 + i));
    }
    const pad = (techno ? 0.006 : 0.022) * duck;
    const hatTime = (beats * 2 % 1) * beatSeconds / 2;
    const hat = (white - this.low) * Math.min(1, hatTime / 0.002) * Math.exp(-hatTime * 65) * (techno ? 0.013 : 0.007);
    const clap = Math.floor(beats) % 2 === 1 ? this.low * Math.min(1, beatTime / 0.003) * Math.exp(-beatTime * 24) * 0.035 : 0;
    const delay = Math.min(this.echo.length - 1, Math.round(beatSeconds * 0.75 * rate));
    const echo = this.echo[(this.position + this.echo.length - delay) % this.echo.length];
    const echoRight = this.echo[(this.position + this.echo.length - Math.min(this.echo.length - 1, delay + Math.round(rate * 0.025))) % this.echo.length];
    this.echo[this.position] = arp * 0.28 + echo * 0.45;
    this.position = (this.position + 1) % this.echo.length;
    const mono = kick + bass * duck + arp + hat + clap;
    this.left = Math.tanh(mono + padLeft * pad + echo * 0.25) * this.fade;
    this.right = Math.tanh(mono + padRight * pad + echoRight * 0.25) * this.fade;
    this.time += 1 / rate;
  }
}
