//! Select the next two-bar groove without allocating on the audio thread.

use super::{Groove, STEPS_PER_BAR};

const GENES: usize = 5;
const CHOICES: [&[i8]; GENES] = [
    &[0, 0, 0, 0, 1, 2],
    &[-1, -1, 0, 0, 7, 12],
    &[0, 1, 1, 2, 3],
    &[0, 0, 0, 1, 2],
    &[-1, -1, 0, 1, 2, 3, 4, 5, 6, 7],
];

type Pattern = [[i8; GENES]; STEPS_PER_BAR];

pub(super) struct Evolution {
    pub(super) pattern: Pattern,
    pub(super) started: bool,
    random: u32,
}

impl Evolution {
    pub(super) fn new(seed: u32) -> Self {
        Self {
            pattern: [[0; GENES]; STEPS_PER_BAR],
            started: false,
            random: seed.max(1),
        }
    }

    pub(super) fn evolve(&mut self, groove: &Groove, arpeggio: &[i8; STEPS_PER_BAR], techno: bool) {
        let anchor = std::array::from_fn(|step| {
            [
                (groove.kick.as_bytes()[step] - b'0') as i8,
                groove.bass[step],
                (groove.hat.as_bytes()[step] - b'0') as i8,
                (groove.clap.as_bytes()[step] - b'0') as i8,
                arpeggio[step],
            ]
        });
        let previous = if self.started { self.pattern } else { anchor };
        let mut parent = previous;
        let mut mate = anchor;
        // Keep the two best parents and breed seven candidates per generation.
        for _ in 0..3 {
            let mut best = parent;
            let mut second = mate;
            let mut highest = fitness(&parent, &anchor, &previous, techno);
            let mut runner_up = i32::MIN;
            for _ in 0..7 {
                let crossover = 2 + self.roll(12);
                let mut child = mate;
                child[..crossover].copy_from_slice(&parent[..crossover]);
                for _ in 0..6 {
                    let step = self.roll(STEPS_PER_BAR);
                    let lane = self.roll(GENES);
                    if (lane == 0 && step % 4 == 0) || (lane == 3 && step % 8 == 4) {
                        continue;
                    }
                    let values = CHOICES[lane];
                    child[step][lane] = values[self.roll(values.len())];
                }
                let rating = fitness(&child, &anchor, &previous, techno);
                if rating > highest {
                    second = best;
                    runner_up = highest;
                    best = child;
                    highest = rating;
                } else if rating > runner_up {
                    second = child;
                    runner_up = rating;
                }
            }
            parent = best;
            mate = second;
        }
        self.pattern = parent;
        self.started = true;
    }

    fn roll(&mut self, limit: usize) -> usize {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        self.random as usize % limit
    }
}

fn fitness(pattern: &Pattern, anchor: &Pattern, previous: &Pattern, techno: bool) -> i32 {
    let mut bass_density = 0_i32;
    let mut arp_density = 0_i32;
    let mut hats = 0_i32;
    let mut ghost_kicks = 0_i32;
    let mut clap_density = 0_i32;
    let mut roots = 0_i32;
    let mut score = 0;
    let mut changed = 0_i32;
    for (step, &[kick, bass, hat, clap, arp]) in pattern.iter().enumerate() {
        let [_, anchor_bass, anchor_hat, anchor_clap, anchor_arp] = anchor[step];
        bass_density += i32::from(bass >= 0) - i32::from(anchor_bass >= 0);
        arp_density += i32::from(arp >= 0) - i32::from(anchor_arp >= 0);
        hats += i32::from(hat > 0) - i32::from(anchor_hat > 0);
        clap_density += i32::from(clap > 0) - i32::from(anchor_clap > 0);
        ghost_kicks += i32::from(step % 4 != 0 && kick > 0);
        roots += i32::from(bass == 0);
        score -= 2 * i32::from(!techno && kick == 3 && bass >= 0);
        score -= i32::from(bass >= 0 && arp >= 0 && clap > 0);
        for (gene, old) in pattern[step].iter().zip(previous[step]) {
            changed += i32::from(*gene != old);
        }
    }
    let novelty_penalty = if changed == 0 {
        90
    } else {
        (changed - 9).abs()
    };
    score
        - 5 * bass_density.abs()
        - 4 * arp_density.abs()
        - 2 * hats.abs()
        - 3 * clap_density.abs()
        - 6 * (ghost_kicks - 2).max(0)
        - 6 * (2 - roots).max(0)
        - novelty_penalty
}
