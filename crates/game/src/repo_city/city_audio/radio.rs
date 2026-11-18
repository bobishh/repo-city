//! Generative radio. Transport, oscillators and echo stay on the audio thread.

use serde::Deserialize;
use std::f32::consts::{PI, TAU};

const RATE: f32 = 44_100.;

#[derive(Deserialize)]
struct Score {
    bpm: [f32; 2],
    chords: [[i16; 4]; 4],
    arpeggio: [usize; 16],
}

pub(super) struct Radio {
    score: Score,
    random: u32,
    station: u8,
    fade: f32,
    time: f64,
    last_step: u64,
    shift: usize,
    low: f32,
    phases: [f32; 4],
    frequencies: [f32; 4],
    kick_phase: f32,
    bass_phase: f32,
    arp_phase: f32,
    arp_frequency: f32,
    target_arp: f32,
    bass_frequency: f32,
    echo: Box<[f32]>,
    position: usize,
}

impl Radio {
    pub(super) fn new(seed: u32) -> Self {
        Self {
            score: serde_json::from_str(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/audio/radio-score.json"
            )))
            .expect("embedded radio score"),
            random: seed.max(1),
            station: 0,
            fade: 0.,
            time: 0.,
            last_step: u64::MAX,
            shift: 0,
            low: 0.,
            phases: [0.; 4],
            frequencies: [0.; 4],
            kick_phase: 0.,
            bass_phase: 0.,
            arp_phase: 0.,
            arp_frequency: 0.,
            target_arp: 0.,
            bass_frequency: 0.,
            echo: vec![0.; RATE as usize * 2].into_boxed_slice(),
            position: 0,
        }
    }

    pub(super) fn frame(&mut self, target: u8) -> [f32; 2] {
        let target = if target <= 2 { target } else { 0 };
        let changing = target != self.station;
        let gain = if changing || target == 0 { 0. } else { 1. };
        self.fade += (gain - self.fade) * (1. - (-1. / (RATE * 0.025)).exp());
        if changing && self.fade < 0.0001 {
            self.station = target;
            self.time = 0.;
            self.last_step = u64::MAX;
        }
        if self.station == 0 {
            return [0.; 2];
        }
        let techno = self.station == 2;
        let beat_seconds = 60. / self.score.bpm[usize::from(self.station - 1)];
        let beats = self.time / f64::from(beat_seconds);
        let step = (beats * 4.).floor() as u64;
        let step_fraction = (beats * 4.).fract() as f32;
        let beat_time = beats.fract() as f32 * beat_seconds;
        let step_time = step_fraction * beat_seconds / 4.;
        let chord = self.score.chords[(step / 32 % 4) as usize];
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        let white = self.random as f32 / u32::MAX as f32 * 2. - 1.;
        self.low += (white - self.low) * 0.12;
        if step != self.last_step {
            if step % 128 == 0 {
                self.shift = (self.random >> 8) as usize % 16;
            }
            for (frequency, note) in self.frequencies.iter_mut().zip(chord) {
                *frequency = pitch(note + 12);
            }
            self.target_arp = pitch(
                chord[self.score.arpeggio[(step as usize + self.shift) % 16]]
                    + if techno { 12 } else { 24 },
            );
            self.bass_frequency = pitch(chord[0] - 12);
            if step % 4 == 0 {
                self.kick_phase = 0.;
            }
            self.last_step = step;
        }
        self.kick_phase =
            (self.kick_phase + (48. + 110. * (-beat_time * 35.).exp()) / RATE).fract();
        self.bass_phase = (self.bass_phase + self.bass_frequency / RATE).fract();
        self.arp_frequency += (self.target_arp - self.arp_frequency) * 0.002;
        self.arp_phase = (self.arp_phase + self.arp_frequency / RATE).fract();
        let kick = (TAU * self.kick_phase).sin()
            * (beat_time / 0.003).min(1.)
            * (-beat_time * 12.).exp()
            * if techno { 0.27 } else { 0.22 };
        let duck = 0.35 + 0.65 * (1. - (-beat_time * 12.).exp());
        let pluck = (PI * step_fraction).sin().max(0.) * (-step_time * 8.).exp();
        let bass_on = if techno {
            matches!(step % 8, 0 | 3 | 6)
        } else {
            step % 4 == 2
        };
        let bass = if bass_on {
            ((TAU * self.bass_phase).sin() + 0.12 * (TAU * 2. * self.bass_phase).sin())
                * pluck
                * 0.13
        } else {
            0.
        };
        let arp = ((TAU * self.arp_phase).sin() + 0.1 * (TAU * 2. * self.arp_phase).sin())
            * pluck
            * if techno { 0.045 } else { 0.085 }
            * duck;
        let mut pads = [0.; 2];
        for (index, phase) in self.phases.iter_mut().enumerate() {
            *phase = (*phase + self.frequencies[index] / RATE).fract();
            pads[0] += (TAU * *phase).sin();
            pads[1] += (TAU * *phase + 0.018 * (self.time as f32 * 0.8 + index as f32).sin()).sin();
        }
        let pad = if techno { 0.006 } else { 0.022 } * duck;
        let hat_time = (beats * 2.).fract() as f32 * beat_seconds / 2.;
        let hat = (white - self.low)
            * (hat_time / 0.002).min(1.)
            * (-hat_time * 65.).exp()
            * if techno { 0.013 } else { 0.007 };
        let clap = if beats.floor() as u64 % 2 == 1 {
            self.low * (beat_time / 0.003).min(1.) * (-beat_time * 24.).exp() * 0.035
        } else {
            0.
        };
        let delay = (beat_seconds * 0.75 * RATE).round() as usize;
        let echo = self.echo[(self.position + self.echo.len() - delay) % self.echo.len()];
        let echo_right = self.echo
            [(self.position + self.echo.len() - delay - (RATE * 0.025) as usize) % self.echo.len()];
        self.echo[self.position] = arp * 0.28 + echo * 0.45;
        self.position = (self.position + 1) % self.echo.len();
        let mono = kick + bass * duck + arp + hat + clap;
        self.time += 1. / f64::from(RATE);
        [
            (mono + pads[0] * pad + echo * 0.25).tanh() * self.fade,
            (mono + pads[1] * pad + echo_right * 0.25).tanh() * self.fade,
        ]
    }
}

fn pitch(note: i16) -> f32 {
    440. * 2_f32.powf(f32::from(note - 69) / 12.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radio_stations_generate_music_and_fade_to_silence() {
        // Given ambience is off, each station still produces bounded musical audio.
        let mut energies = Vec::new();
        for station in [1, 2] {
            let mut radio = Radio::new(123);
            let mut energy = 0_f64;
            let mut previous = 0_f32;
            let mut largest_step = 0_f32;
            for _ in 0..44_100 * 4 {
                let [left, right] = radio.frame(station);
                assert!(left.is_finite() && right.is_finite());
                assert!(left.abs() < 0.8 && right.abs() < 0.8);
                largest_step = largest_step.max((left - previous).abs());
                previous = left;
                energy += f64::from(left * left);
            }
            assert!(energy > 1.);
            assert!(largest_step < 0.16, "sample step {largest_step}");
            energies.push(energy);
            for _ in 0..44_100 {
                radio.frame(0);
            }
            assert!(
                radio
                    .frame(0)
                    .into_iter()
                    .all(|value| value.abs() < 0.00001)
            );
        }
        assert_ne!(energies[0], energies[1]);
    }
}
