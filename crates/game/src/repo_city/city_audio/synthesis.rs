//! Bounded procedural city sounds. The audio callback performs no allocation or locking.

const RATE: f32 = 44_100.;
use super::radio::Radio;

#[derive(Clone, Copy, Default)]
pub(super) struct Levels {
    pub master: f32,
    pub wind: f32,
    pub spray: f32,
    pub works: f32,
    pub siren: f32,
    pub pan: f32,
    pub music: f32,
    pub radio: f32,
}

pub(super) struct Synth {
    random: u32,
    frame: u64,
    low: f32,
    air: f32,
    breeze: f32,
    impact: f32,
    impact_target: f32,
    thud_phase: f32,
    phase: f32,
    levels: Levels,
    radio: Radio,
}

impl Synth {
    pub fn new(seed: u32) -> Self {
        Self {
            random: seed.max(1),
            frame: 0,
            low: 0.,
            air: 0.,
            breeze: 0.,
            impact: 0.,
            impact_target: 0.,
            thud_phase: 0.,
            phase: 0.,
            levels: Levels::default(),
            radio: Radio::new(seed),
        }
    }

    pub fn frame(&mut self, target: Levels) -> [f32; 2] {
        let smooth = |current: &mut f32, target: f32| *current += (target - *current) * 0.0004;
        smooth(&mut self.levels.master, target.master);
        smooth(&mut self.levels.wind, target.wind);
        smooth(&mut self.levels.spray, target.spray);
        smooth(&mut self.levels.works, target.works);
        smooth(&mut self.levels.siren, target.siren);
        smooth(&mut self.levels.pan, target.pan);
        smooth(&mut self.levels.music, target.music);
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        let white = self.random as f32 / u32::MAX as f32 * 2. - 1.;
        self.low += (white - self.low) * 0.008;
        self.air += (white - self.air) * 0.06;
        self.breeze += (self.low - self.breeze) * 0.04;
        // Colored air and band-limited water avoid broadband static at the listener.
        let wind = self.breeze * 0.9 * self.levels.wind;
        let spray = (self.air - self.low) * 0.085 * self.levels.spray;
        // Distant soft thuds: finite attack/release, no instantaneous noise impulses.
        let cycle = self.frame % (44_100 * 31);
        if cycle < 44_100 * 2 && cycle % 21_609 == 0 {
            self.impact_target = 1.;
        }
        self.impact_target *= 0.9995;
        self.impact += (self.impact_target - self.impact) * 0.001;
        self.thud_phase = (self.thud_phase + 73. / RATE).fract();
        let works = self.impact
            * ((std::f32::consts::TAU * self.thud_phase).sin() * 0.045 + self.air * 0.025)
            * self.levels.works;
        let siren_time = (self.frame % (44_100 * 97)) as f32 / RATE;
        self.phase = (self.phase + (510. + 110. * (siren_time * 1.3).sin()) / RATE).fract();
        let envelope = if siren_time < 5. {
            (std::f32::consts::PI * siren_time / 5.)
                .sin()
                .max(0.)
                .powi(2)
        } else {
            0.
        };
        let siren =
            (std::f32::consts::TAU * self.phase).sin() * envelope * 0.015 * self.levels.siren;
        self.frame = self.frame.wrapping_add(1);
        let mixed = (wind + spray + works + siren) * self.levels.master;
        let pan = self.levels.pan.clamp(-1., 1.);
        let music = self.radio.frame(target.radio as u8);
        [
            (mixed * (1. - 0.4 * pan) + music[0] * self.levels.music).clamp(-1., 1.),
            (mixed * (1. + 0.4 * pan) + music[1] * self.levels.music).clamp(-1., 1.),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given a city with every layer enabled, when audio advances for one minute,
    // output stays finite, bounded and deterministic, including abrupt mute changes.
    #[test]
    fn audible_layers_stay_below_a_transient_click_budget() {
        let mut synth = Synth::new(123);
        let levels = Levels {
            master: 0.4,
            wind: 1.,
            spray: 1.,
            works: 1.,
            siren: 1.,
            pan: 0.6,
            ..Levels::default()
        };
        let mut previous = [0.; 2];
        let mut largest = 0_f32;
        let mut difference_energy = 0_f64;
        for _ in 0..88_200 {
            let frame = synth.frame(levels);
            for channel in 0..2 {
                let difference = frame[channel] - previous[channel];
                largest = largest.max(difference.abs());
                difference_energy += f64::from(difference * difference);
            }
            previous = frame;
        }
        let rms = (difference_energy / 176_400.).sqrt();
        assert!(
            largest < 0.008 && rms < 0.0015,
            "sample step {largest:.6}, difference RMS {rms:.6}"
        );
    }

    #[test]
    fn procedural_audio_is_bounded_repeatable_and_mutes() {
        let levels = Levels {
            master: 1.,
            wind: 1.,
            spray: 1.,
            works: 1.,
            siren: 1.,
            pan: -0.6,
            ..Levels::default()
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
