//! Generative radio. Transport, oscillators and echo stay on the audio thread.

mod evolution;

use std::f32::consts::TAU;

use evolution::Evolution;
use serde::Deserialize;

const RATE: f32 = 44_100.;
const STEPS_PER_BAR: usize = 16;
const BARS_PER_SECTION: usize = 4;

#[derive(Deserialize)]
struct Score {
    grooves: Vec<Groove>,
    stations: [Station; 2],
}

#[derive(Deserialize)]
struct Groove {
    kick: String,
    bass: [i8; STEPS_PER_BAR],
    hat: String,
    clap: String,
}

#[derive(Deserialize)]
struct Station {
    bpm: f64,
    chords: Vec<[i16; 5]>,
    arps: Vec<[i8; STEPS_PER_BAR]>,
    leads: Vec<[i8; STEPS_PER_BAR]>,
    sections: Vec<Section>,
}

#[derive(Deserialize)]
struct Section {
    chords: [usize; BARS_PER_SECTION],
    groove: usize,
    arp: [usize; 2],
    lead: [usize; 2],
    kick: f32,
    bass: f32,
    hat: f32,
    clap: f32,
    pad: f32,
    pluck: f32,
    hook: f32,
}

#[derive(Default)]
struct Envelope {
    remaining: f32,
    gain: f32,
}

impl Envelope {
    fn strike(&mut self, velocity: f32) {
        self.remaining = velocity;
    }

    fn advance(&mut self, attack: f32, release: f32) -> f32 {
        self.gain += (self.remaining - self.gain) * attack;
        self.remaining *= release;
        self.gain
    }
}

#[derive(Default)]
struct Note {
    phase: f32,
    frequency: f32,
    target: f32,
    envelope: Envelope,
}

impl Note {
    fn strike(&mut self, pitch: i16, velocity: f32) {
        self.target = frequency(pitch);
        if self.frequency == 0. {
            self.frequency = self.target;
        }
        self.envelope.strike(velocity);
    }

    fn advance(&mut self, glide: f32) -> f32 {
        self.frequency += (self.target - self.frequency) * glide;
        self.phase = (self.phase + self.frequency / RATE).fract();
        TAU * self.phase
    }
}

struct Timing {
    fade: f32,
    attack: f32,
    glide: f32,
    chord_glide: f32,
    pad_attack: f32,
    kick: f32,
    sweep: f32,
    closed_hat: f32,
    open_hat: f32,
    clap: f32,
    bass: [f32; 2],
    arp: [f32; 2],
    lead: [f32; 2],
}

impl Timing {
    fn new() -> Self {
        Self {
            fade: 1. - decay(0.025),
            attack: 1. - decay(0.0025),
            glide: 1. - decay(0.009),
            chord_glide: 1. - decay(0.045),
            pad_attack: 1. - decay(0.2),
            kick: decay(0.085),
            sweep: decay(0.043),
            closed_hat: decay(0.024),
            open_hat: decay(0.08),
            clap: decay(0.05),
            bass: [decay(0.075), decay(0.092)],
            arp: [decay(0.13), decay(0.078)],
            lead: [decay(0.32), decay(0.16)],
        }
    }
}

pub(super) struct Radio {
    score: Score,
    timing: Timing,
    evolution: Evolution,
    seed: u32,
    random: u32,
    station: u8,
    fade: f32,
    time: f64,
    steps_per_second: f64,
    last_step: usize,
    kick: Envelope,
    hat: Envelope,
    clap: Envelope,
    kick_phase: f32,
    kick_sweep: f32,
    hat_release: f32,
    noise_low: f32,
    noise_body: f32,
    bass: Note,
    arp: Note,
    lead: Note,
    pad_phases: [[f32; 4]; 2],
    pad_frequency: [f32; 4],
    pad_pitch: [f32; 4],
    pad_gain: f32,
    pad_target: f32,
    echo: Box<[f32]>,
    echo_age: usize,
    delay: usize,
    delay_right: usize,
    position: usize,
}

impl Radio {
    pub(super) fn new(seed: u32) -> Self {
        let timing = Timing::new();
        Self {
            score: serde_json::from_str(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/audio/radio-score.json"
            )))
            .expect("embedded radio score"),
            evolution: Evolution::new(seed),
            seed: seed.max(1),
            random: seed.max(1),
            station: 0,
            fade: 0.,
            time: 0.,
            steps_per_second: 0.,
            last_step: usize::MAX,
            kick: Envelope::default(),
            hat: Envelope::default(),
            clap: Envelope::default(),
            kick_phase: 0.,
            kick_sweep: 0.,
            hat_release: timing.closed_hat,
            noise_low: 0.,
            noise_body: 0.,
            bass: Note::default(),
            arp: Note::default(),
            lead: Note::default(),
            pad_phases: [[0.; 4]; 2],
            pad_frequency: [0.; 4],
            pad_pitch: [0.; 4],
            pad_gain: 0.,
            pad_target: 0.,
            echo: vec![0.; RATE as usize * 2].into_boxed_slice(),
            echo_age: 0,
            delay: 0,
            delay_right: 0,
            position: 0,
            timing,
        }
    }

    fn tune(&mut self, station: u8) {
        self.station = station;
        self.time = 0.;
        self.last_step = usize::MAX;
        self.echo_age = 0;
        self.pad_gain = 0.;
        self.pad_frequency = [0.; 4];
        self.evolution.started = false;
        self.kick = Envelope::default();
        self.hat = Envelope::default();
        self.clap = Envelope::default();
        self.bass.envelope = Envelope::default();
        self.arp.envelope = Envelope::default();
        self.lead.envelope = Envelope::default();
        self.hat_release = self.timing.closed_hat;
        if station != 0 {
            let beat = 60. / self.score.stations[usize::from(station - 1)].bpm;
            self.steps_per_second = 4. / beat;
            self.delay = (beat * 0.75 * f64::from(RATE)).round() as usize;
            self.delay_right = (beat * 1.25 * f64::from(RATE)).round() as usize;
        }
    }

    fn play_step(&mut self, absolute_step: usize) {
        let techno = self.station == 2;
        let station = &self.score.stations[usize::from(self.station - 1)];
        let bar = absolute_step / STEPS_PER_BAR;
        let section_number = bar / BARS_PER_SECTION;
        let bar_in_section = bar % BARS_PER_SECTION;
        let step = absolute_step % STEPS_PER_BAR;
        let section = &station.sections[section_number % station.sections.len()];
        let variation =
            (bar + (self.seed >> 8) as usize + section_number / station.sections.len()) % 2;
        let chord = station.chords[section.chords[bar_in_section]];
        if step == 0 && bar % 2 == 0 {
            let motif = section.arp[(variation + bar_in_section / 2) % 2];
            self.evolution.evolve(
                &self.score.grooves[section.groove],
                &station.arps[motif],
                techno,
            );
        }
        let [kick, bass, hat, clap, arp] = self.evolution.pattern[step];
        if step == 0 {
            for (index, &note) in chord[1..].iter().enumerate() {
                self.pad_pitch[index] = frequency(note);
                if self.pad_frequency[index] == 0. {
                    self.pad_frequency[index] = self.pad_pitch[index];
                }
            }
            self.pad_target = section.pad;
        }
        let kick = f32::from(kick) / 3. * section.kick;
        if kick > 0. {
            self.kick.strike(kick);
            self.kick_sweep = 1.;
        }
        if hat > 0 {
            self.hat.strike(f32::from(hat) / 3. * section.hat);
            self.hat_release = if hat == 3 {
                self.timing.open_hat
            } else {
                self.timing.closed_hat
            };
        }
        let fill = match (bar_in_section == BARS_PER_SECTION - 1, step) {
            (true, 13) => 0.38,
            (true, 15) => 0.75,
            _ => 0.,
        };
        let clap = (f32::from(clap) / 3.).max(fill);
        if clap > 0. {
            self.clap.strike(clap * section.clap);
        }
        if bass >= 0 {
            self.bass
                .strike(chord[0] - 12 + i16::from(bass), section.bass);
        }
        if arp >= 0 {
            let octave = if techno { 0 } else { 12 };
            let accent = if step % 4 == 0 { 1. } else { 0.76 };
            self.arp
                .strike(chord_pitch(chord, arp) + octave, section.pluck * accent);
        }
        let lead = station.leads[section.lead[variation]][step];
        if lead >= 0 {
            self.lead.strike(chord_pitch(chord, lead), section.hook);
        }
    }

    pub(super) fn frame(&mut self, target: u8) -> [f32; 2] {
        let target = if target <= 2 { target } else { 0 };
        let changing = target != self.station;
        let gain = if changing || target == 0 { 0. } else { 1. };
        self.fade += (gain - self.fade) * self.timing.fade;
        if changing && self.fade < 0.0001 {
            self.tune(target);
        }
        if self.station == 0 {
            return [0.; 2];
        }
        let step = (self.time * self.steps_per_second).floor() as usize;
        if step != self.last_step {
            self.play_step(step);
            self.last_step = step;
        }
        let station = usize::from(self.station - 1);
        let techno = station == 1;
        let timing = &self.timing;
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        let white = self.random as f32 / u32::MAX as f32 * 2. - 1.;
        self.noise_low += (white - self.noise_low) * 0.18;
        self.noise_body += (self.noise_low - self.noise_body) * 0.034;
        self.kick_phase =
            (self.kick_phase + (47. + 155. * self.kick_sweep * self.kick_sweep) / RATE).fract();
        self.kick_sweep *= timing.sweep;
        let kick_gain = self.kick.advance(timing.attack, timing.kick);
        let kick = (TAU * self.kick_phase).sin() * kick_gain * if techno { 0.33 } else { 0.28 };
        let duck = 1. - kick_gain * 0.66;
        let hat = (white - self.noise_low)
            * self.hat.advance(timing.attack, self.hat_release)
            * if techno { 0.049 } else { 0.031 };
        let clap = (0.35 * (white - self.noise_body) + 0.65 * self.noise_low)
            * self.clap.advance(timing.attack, timing.clap)
            * 0.12;
        let phase = self.bass.advance(timing.attack);
        let bass = (phase.sin() + 0.24 * (2. * phase).sin() + 0.12 * (3. * phase).sin())
            * self
                .bass
                .envelope
                .advance(timing.attack, timing.bass[station])
            * if techno { 0.17 } else { 0.14 }
            * duck;
        let phase = self.arp.advance(timing.glide);
        let arp = (phase.sin() + 0.22 * (3. * phase).sin() + 0.1 * (5. * phase).sin())
            * self
                .arp
                .envelope
                .advance(timing.attack, timing.arp[station])
            * if techno { 0.075 } else { 0.048 }
            * duck;
        let phase = self.lead.advance(timing.glide);
        let wave = phase.sin();
        let lead = (wave + 0.33 * (2. * phase + 0.5 * wave).sin() + 0.13 * (3. * phase).sin())
            * self
                .lead
                .envelope
                .advance(timing.attack, timing.lead[station])
            * if techno { 0.105 } else { 0.093 }
            * duck;

        let mut pads = [0.; 2];
        for (index, frequency) in self.pad_frequency.iter_mut().enumerate() {
            *frequency += (self.pad_pitch[index] - *frequency) * timing.chord_glide;
            let increment = *frequency / RATE;
            self.pad_phases[0][index] = (self.pad_phases[0][index] + increment).fract();
            let detune = if index % 2 == 0 { 1.003 } else { 0.997 };
            self.pad_phases[1][index] = (self.pad_phases[1][index] + increment * detune).fract();
            for (pad, phases) in pads.iter_mut().zip(&self.pad_phases) {
                let phase = TAU * phases[index];
                *pad += phase.sin() + 0.18 * (2. * phase).sin();
            }
        }
        self.pad_gain += (self.pad_target - self.pad_gain) * timing.pad_attack;
        let pad = self.pad_gain * if techno { 0.022 } else { 0.037 } * duck;
        let echo = self.echo_at(self.delay);
        let echo_right = self.echo_at(self.delay_right);
        self.echo[self.position] = arp * 0.42 + lead * 0.5 + echo * 0.43;
        self.position = (self.position + 1) % self.echo.len();
        self.echo_age = (self.echo_age + 1).min(self.echo.len());
        let mono = kick + bass + arp + lead + hat + clap;
        self.time += 1. / f64::from(RATE);
        [
            (mono + pads[0] * pad + echo * 0.47).tanh() * self.fade,
            (mono + pads[1] * pad + echo_right * 0.47).tanh() * self.fade,
        ]
    }

    fn echo_at(&self, delay: usize) -> f32 {
        if self.echo_age >= delay {
            self.echo[(self.position + self.echo.len() - delay) % self.echo.len()]
        } else {
            0.
        }
    }
}

fn chord_pitch(chord: [i16; 5], note: i8) -> i16 {
    chord[1 + (note % 4) as usize] + if note >= 4 { 12 } else { 0 }
}

fn decay(seconds: f32) -> f32 {
    (-1. / (RATE * seconds)).exp()
}

fn frequency(note: i16) -> f32 {
    440. * 2_f32.powf(f32::from(note - 69) / 12.)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radio_evolves_the_same_phrases_as_the_browser() {
        // The browser synth produces these fingerprints for the first 48 phrases.
        for (station, expected) in [(1, 816_440_194), (2, 3_450_616_615)] {
            let mut radio = Radio::new(123);
            radio.tune(station);
            let mut fingerprint = 2_166_136_261_u32;
            for phrase in 0..48 {
                radio.play_step(phrase * STEPS_PER_BAR * 2);
                for gene in radio.evolution.pattern.into_iter().flatten() {
                    fingerprint =
                        (fingerprint ^ u32::from(gene.cast_unsigned())).wrapping_mul(16_777_619);
                }
            }
            assert_eq!(fingerprint, expected);
        }
    }

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
