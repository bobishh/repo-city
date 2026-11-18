//! Bounded procedural city sounds. The audio callback performs no allocation or locking.

const RATE: f32 = 44_100.;

#[derive(Clone, Copy, Default)]
pub(super) struct Levels {
    pub master: f32,
    pub wind: f32,
    pub spray: f32,
    pub works: f32,
    pub siren: f32,
    pub pan: f32,
}

pub(super) struct Synth {
    random: u32,
    frame: u64,
    low: f32,
    air: f32,
    impact: f32,
    phase: f32,
    levels: Levels,
}

impl Synth {
    pub fn new(seed: u32) -> Self {
        Self {
            random: seed.max(1),
            frame: 0,
            low: 0.,
            air: 0.,
            impact: 0.,
            phase: 0.,
            levels: Levels::default(),
        }
    }

    pub fn frame(&mut self, target: Levels) -> [f32; 2] {
        let smooth = |current: &mut f32, target: f32| *current += (target - *current) * 0.0008;
        smooth(&mut self.levels.master, target.master);
        smooth(&mut self.levels.wind, target.wind);
        smooth(&mut self.levels.spray, target.spray);
        smooth(&mut self.levels.works, target.works);
        smooth(&mut self.levels.siren, target.siren);
        smooth(&mut self.levels.pan, target.pan);
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        let white = self.random as f32 / u32::MAX as f32 * 2. - 1.;
        self.low += (white - self.low) * 0.008;
        self.air += (white - self.air) * 0.14;
        let wind = self.low * 3.5 * self.levels.wind;
        let spray = (white - self.air) * 0.17 * self.levels.spray;
        // Short clusters of muted impacts, separated by long quiet intervals.
        let cycle = self.frame % (44_100 * 13);
        if cycle < 44_100 * 3 && cycle % 7_613 == 0 {
            self.impact = 1.;
        }
        self.impact *= 0.997;
        let works = self.impact * (self.air * 0.8 + white * 0.05) * self.levels.works;
        // A passing siren, not a constant alarm. Both oscillators retain phase.
        let siren_cycle = self.frame % (44_100 * 47);
        let siren_time = siren_cycle as f32 / RATE;
        let sweep = (siren_time * 1.7).sin();
        self.phase = (self.phase + (590. + 170. * sweep) / RATE).fract();
        let envelope = if siren_time < 7. {
            (std::f32::consts::PI * siren_time / 7.).sin().max(0.)
        } else {
            0.
        };
        let siren =
            (std::f32::consts::TAU * self.phase).sin() * envelope * 0.065 * self.levels.siren;
        self.frame = self.frame.wrapping_add(1);
        let mixed = (wind + spray + works + siren) * self.levels.master;
        let pan = self.levels.pan.clamp(-1., 1.);
        [
            (mixed * (1. - 0.4 * pan)).clamp(-1., 1.),
            (mixed * (1. + 0.4 * pan)).clamp(-1., 1.),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given a city with every layer enabled, when audio advances for one minute,
    // output stays finite, bounded and deterministic, including abrupt mute changes.
    #[test]
    fn procedural_audio_is_bounded_repeatable_and_mutes() {
        let levels = Levels {
            master: 1.,
            wind: 1.,
            spray: 1.,
            works: 1.,
            siren: 1.,
            pan: -0.6,
        };
        let mut first = Synth::new(123);
        let mut second = Synth::new(123);
        let mut energy = 0.;
        for _ in 0..(RATE as usize * 60) {
            let a = first.frame(levels);
            assert_eq!(a, second.frame(levels));
            assert!(
                a.into_iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= 1.)
            );
            energy += a[0].abs();
        }
        assert!(energy > 100.);
        for _ in 0..44_100 {
            first.frame(Levels::default());
        }
        assert!(
            first
                .frame(Levels::default())
                .into_iter()
                .all(|sample| sample.abs() < 0.0001)
        );
    }
}
