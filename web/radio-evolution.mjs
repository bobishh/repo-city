// Each sixteenth note carries kick, bass, hat, clap and arpeggio genes.
const STEPS = 16;
const LANES = 5;
const CHOICES = [[0, 0, 0, 0, 1, 2], [-1, -1, 0, 0, 7, 12], [0, 1, 1, 2, 3], [0, 0, 0, 1, 2], [-1, -1, 0, 1, 2, 3, 4, 5, 6, 7]];

export class Evolution {
  constructor(seed) {
    this.random = seed || 1;
    this.pattern = new Int8Array(STEPS * LANES);
    this.anchor = new Int8Array(STEPS * LANES);
    this.parent = new Int8Array(STEPS * LANES);
    this.mate = new Int8Array(STEPS * LANES);
    this.best = new Int8Array(STEPS * LANES);
    this.second = new Int8Array(STEPS * LANES);
    this.child = new Int8Array(STEPS * LANES);
    this.started = false;
  }

  roll(limit) {
    this.random ^= this.random << 13;
    this.random ^= this.random >>> 17;
    this.random ^= this.random << 5;
    return (this.random >>> 0) % limit;
  }

  evolve(groove, arpeggio, techno) {
    for (let step = 0; step < STEPS; step++) {
      const i = step * LANES;
      this.anchor[i] = groove.kick.charCodeAt(step) - 48;
      this.anchor[i + 1] = groove.bass[step];
      this.anchor[i + 2] = groove.hat.charCodeAt(step) - 48;
      this.anchor[i + 3] = groove.clap.charCodeAt(step) - 48;
      this.anchor[i + 4] = arpeggio[step];
    }
    this.parent.set(this.started ? this.pattern : this.anchor);
    this.mate.set(this.anchor);
    for (let generation = 0; generation < 3; generation++) {
      let highest = this.fitness(this.parent, techno), runnerUp = -Infinity;
      this.best.set(this.parent);
      for (let offspring = 0; offspring < 7; offspring++) {
        const crossover = (2 + this.roll(12)) * LANES;
        for (let i = 0; i < this.child.length; i++) {
          this.child[i] = (i < crossover ? this.parent : this.mate)[i];
        }
        for (let mutation = 0; mutation < 6; mutation++) {
          const step = this.roll(STEPS), lane = this.roll(LANES);
          if ((lane === 0 && step % 4 === 0) || (lane === 3 && step % 8 === 4)) continue;
          const choices = CHOICES[lane];
          this.child[step * LANES + lane] = choices[this.roll(choices.length)];
        }
        const rating = this.fitness(this.child, techno);
        if (rating > highest) {
          this.second.set(this.best);
          runnerUp = highest;
          this.best.set(this.child);
          highest = rating;
        } else if (rating > runnerUp) {
          this.second.set(this.child);
          runnerUp = rating;
        }
      }
      this.parent.set(this.best);
      this.mate.set(this.second);
    }
    this.pattern.set(this.parent);
    this.started = true;
  }

  fitness(pattern, techno) {
    let bassDensity = 0, arpDensity = 0, hats = 0, ghostKicks = 0;
    let clapDensity = 0, roots = 0, score = 0, changed = 0;
    for (let step = 0; step < STEPS; step++) {
      const i = step * LANES;
      const kick = pattern[i], bass = pattern[i + 1], arp = pattern[i + 4];
      bassDensity += Number(bass >= 0) - Number(this.anchor[i + 1] >= 0);
      arpDensity += Number(arp >= 0) - Number(this.anchor[i + 4] >= 0);
      hats += Number(pattern[i + 2] > 0) - Number(this.anchor[i + 2] > 0);
      clapDensity += Number(pattern[i + 3] > 0) - Number(this.anchor[i + 3] > 0);
      if (step % 4 !== 0 && kick > 0) ghostKicks++;
      if (bass === 0) roots++;
      if (!techno && kick === 3 && bass >= 0) score -= 2;
      if (bass >= 0 && arp >= 0 && pattern[i + 3] > 0) score--;
      for (let lane = 0; lane < LANES; lane++) {
        changed += Number(pattern[i + lane] !== (this.started ? this.pattern : this.anchor)[i + lane]);
      }
    }
    return score - 5 * Math.abs(bassDensity) - 4 * Math.abs(arpDensity) - 2 * Math.abs(hats) -
      3 * Math.abs(clapDensity) - 6 * Math.max(0, ghostKicks - 2) - 6 * Math.max(0, 2 - roots) -
      (changed === 0 ? 90 : Math.abs(changed - 9));
  }
}
