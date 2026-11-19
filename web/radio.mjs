import score from "../assets/audio/radio-score.json" with { type: "json" };
import { Evolution } from "./radio-evolution.mjs";

const TAU = Math.PI * 2;
const STEPS_PER_BAR = 16;
const BARS_PER_SECTION = 4;
const pitch = note => 440 * 2 ** ((note - 69) / 12);
const decay = (rate, seconds) => Math.exp(-1 / (rate * seconds));
const smooth = (rate, seconds) => 1 - decay(rate, seconds);

class Envelope {
  remaining = 0;
  gain = 0;

  strike(velocity) { this.remaining = velocity; }
  reset() { this.remaining = this.gain = 0; }

  advance(attack, release) {
    this.gain += (this.remaining - this.gain) * attack;
    this.remaining *= release;
    return this.gain;
  }
}

class Note {
  phase = 0;
  frequency = 0;
  target = 0;
  envelope = new Envelope();

  strike(note, velocity) {
    this.target = pitch(note);
    if (!this.frequency) this.frequency = this.target;
    this.envelope.strike(velocity);
  }

  advance(rate, glide) {
    this.frequency += (this.target - this.frequency) * glide;
    this.phase = (this.phase + this.frequency / rate) % 1;
    return TAU * this.phase;
  }
}

// Only the score and the different oscillator colors vary by station.
export class RadioSynth {
  constructor(seed = 0xc17a) {
    this.seed = this.random = seed || 1;
    this.evolution = new Evolution(this.seed);
    this.station = this.rate = this.fade = this.time = this.padGain = this.padTarget = 0;
    this.lastStep = -1;
    this.kickPhase = this.kickSweep = this.noiseLow = this.noiseBody = 0;
    this.bass = new Note();
    this.arp = new Note();
    this.lead = new Note();
    this.kick = new Envelope();
    this.hat = new Envelope();
    this.clap = new Envelope();
    this.padPhase = new Float64Array(4);
    this.padPhaseRight = new Float64Array(4);
    this.padFrequency = new Float64Array(4);
    this.padPitch = new Float64Array(4);
    this.echo = new Float32Array(192000);
    this.position = this.echoAge = this.left = this.right = 0;
  }

  configure(rate) {
    this.rate = rate;
    this.fadeSpeed = smooth(rate, 0.025);
    this.attack = smooth(rate, 0.0025);
    this.glide = smooth(rate, 0.009);
    this.chordGlide = smooth(rate, 0.045);
    this.padAttack = smooth(rate, 0.2);
    this.kickRelease = decay(rate, 0.085);
    this.sweepRelease = decay(rate, 0.043);
    this.closedHat = decay(rate, 0.024);
    this.openHat = decay(rate, 0.08);
    this.clapRelease = decay(rate, 0.05);
    this.bassRelease = decay(rate, this.station === 2 ? 0.092 : 0.075);
    this.arpRelease = decay(rate, this.station === 2 ? 0.078 : 0.13);
    this.leadRelease = decay(rate, this.station === 2 ? 0.16 : 0.32);
    if (!this.station) return;
    const beat = 60 / score.stations[this.station - 1].bpm;
    this.stepsPerSecond = 4 / beat;
    this.delay = Math.min(this.echo.length - 1, Math.round(rate * beat * 0.75));
    this.delayRight = Math.min(this.echo.length - 1, Math.round(rate * beat * 1.25));
  }

  playStep(absoluteStep) {
    const station = score.stations[this.station - 1];
    const bar = Math.floor(absoluteStep / STEPS_PER_BAR);
    const sectionNumber = Math.floor(bar / BARS_PER_SECTION);
    const barInSection = bar % BARS_PER_SECTION;
    const step = absoluteStep % STEPS_PER_BAR;
    const section = station.sections[sectionNumber % station.sections.length];
    const variation = (bar + (this.seed >>> 8) + Math.floor(sectionNumber / station.sections.length)) % 2;
    const chord = station.chords[section.chords[barInSection]];
    if (step === 0 && bar % 2 === 0) {
      const motif = section.arp[(variation + barInSection / 2) % 2];
      this.evolution.evolve(score.grooves[section.groove], station.arps[motif], this.station === 2);
    }
    const genes = this.evolution.pattern;
    const offset = step * 5;
    if (step === 0) {
      for (let index = 0; index < 4; index++) {
        this.padPitch[index] = pitch(chord[index + 1]);
        if (!this.padFrequency[index]) this.padFrequency[index] = this.padPitch[index];
      }
      this.padTarget = section.pad;
    }

    const kick = genes[offset] / 3 * section.kick;
    if (kick > 0) {
      this.kick.strike(kick);
      this.kickSweep = 1;
    }
    const hat = genes[offset + 2] / 3;
    if (hat > 0) {
      this.hat.strike(hat * section.hat);
      this.hatRelease = hat === 1 ? this.openHat : this.closedHat;
    }
    const fill = barInSection === BARS_PER_SECTION - 1 && (step === 13 || step === 15);
    const clap = Math.max(genes[offset + 3] / 3, fill ? (step === 15 ? 0.75 : 0.38) : 0);
    if (clap > 0) this.clap.strike(clap * section.clap);

    const bass = genes[offset + 1];
    if (bass >= 0) this.bass.strike(chord[0] - 12 + bass, section.bass);
    const arp = genes[offset + 4];
    if (arp >= 0) {
      const octave = this.station === 2 ? 0 : 12;
      this.arp.strike(chord[1 + arp % 4] + octave + (arp >= 4 ? 12 : 0), section.pluck * (step % 4 === 0 ? 1 : 0.76));
    }
    const lead = station.leads[section.lead[variation]][step];
    if (lead >= 0) this.lead.strike(chord[1 + lead % 4] + (lead >= 4 ? 12 : 0), section.hook);
  }

  tick(target, rate) {
    if (rate !== this.rate) this.configure(rate);
    const changing = target !== this.station;
    this.fade += ((changing || target === 0 ? 0 : 1) - this.fade) * this.fadeSpeed;
    if (changing && this.fade < 0.0001) {
      this.station = target;
      this.evolution.started = false;
      this.time = this.echoAge = this.padGain = 0;
      this.lastStep = -1;
      this.padFrequency.fill(0);
      this.kick.reset(); this.hat.reset(); this.clap.reset();
      this.bass.envelope.reset(); this.arp.envelope.reset(); this.lead.envelope.reset();
      this.hatRelease = this.closedHat;
      this.configure(rate);
    }
    if (!this.station) { this.left = this.right = 0; return; }

    const step = Math.floor(this.time * this.stepsPerSecond);
    if (step !== this.lastStep) {
      this.playStep(step);
      this.lastStep = step;
    }
    const techno = this.station === 2;
    this.random ^= this.random << 13; this.random ^= this.random >>> 17; this.random ^= this.random << 5;
    const white = (this.random >>> 0) / 4294967295 * 2 - 1;
    this.noiseLow += (white - this.noiseLow) * 0.18;
    this.noiseBody += (this.noiseLow - this.noiseBody) * 0.034;
    this.kickPhase = (this.kickPhase + (47 + 155 * this.kickSweep * this.kickSweep) / rate) % 1;
    this.kickSweep *= this.sweepRelease;
    const kickGain = this.kick.advance(this.attack, this.kickRelease);
    const kick = Math.sin(TAU * this.kickPhase) * kickGain * (techno ? 0.33 : 0.28);
    const duck = 1 - kickGain * 0.66;
    const hat = (white - this.noiseLow) * this.hat.advance(this.attack, this.hatRelease) * (techno ? 0.049 : 0.031);
    const clap = (0.35 * (white - this.noiseBody) + 0.65 * this.noiseLow) * this.clap.advance(this.attack, this.clapRelease) * 0.12;
    const bassPhase = this.bass.advance(rate, this.attack);
    const bass = (Math.sin(bassPhase) + 0.24 * Math.sin(2 * bassPhase) + 0.12 * Math.sin(3 * bassPhase)) *
      this.bass.envelope.advance(this.attack, this.bassRelease) * (techno ? 0.17 : 0.14) * duck;
    const arpPhase = this.arp.advance(rate, this.glide);
    const arp = (Math.sin(arpPhase) + 0.22 * Math.sin(3 * arpPhase) + 0.1 * Math.sin(5 * arpPhase)) *
      this.arp.envelope.advance(this.attack, this.arpRelease) * (techno ? 0.075 : 0.048) * duck;
    const leadPhase = this.lead.advance(rate, this.glide);
    const leadWave = Math.sin(leadPhase);
    const lead = (leadWave + 0.33 * Math.sin(2 * leadPhase + 0.5 * leadWave) + 0.13 * Math.sin(3 * leadPhase)) *
      this.lead.envelope.advance(this.attack, this.leadRelease) * (techno ? 0.105 : 0.093) * duck;

    let padLeft = 0, padRight = 0;
    for (let index = 0; index < 4; index++) {
      this.padFrequency[index] += (this.padPitch[index] - this.padFrequency[index]) * this.chordGlide;
      const increment = this.padFrequency[index] / rate;
      this.padPhase[index] = (this.padPhase[index] + increment) % 1;
      this.padPhaseRight[index] = (this.padPhaseRight[index] + increment * (index % 2 ? 0.997 : 1.003)) % 1;
      const left = TAU * this.padPhase[index], right = TAU * this.padPhaseRight[index];
      padLeft += Math.sin(left) + 0.18 * Math.sin(left * 2);
      padRight += Math.sin(right) + 0.18 * Math.sin(right * 2);
    }
    this.padGain += (this.padTarget - this.padGain) * this.padAttack;
    const pad = this.padGain * (techno ? 0.022 : 0.037) * duck;

    const echo = this.echoAge >= this.delay ? this.echo[(this.position + this.echo.length - this.delay) % this.echo.length] : 0;
    const echoRight = this.echoAge >= this.delayRight ? this.echo[(this.position + this.echo.length - this.delayRight) % this.echo.length] : 0;
    this.echo[this.position] = arp * 0.42 + lead * 0.5 + echo * 0.43;
    this.position = (this.position + 1) % this.echo.length;
    this.echoAge = Math.min(this.echo.length, this.echoAge + 1);
    const mono = kick + bass + arp + lead + hat + clap;
    this.left = Math.tanh(mono + padLeft * pad + echo * 0.47) * this.fade;
    this.right = Math.tanh(mono + padRight * pad + echoRight * 0.47) * this.fade;
    this.time += 1 / rate;
  }
}
