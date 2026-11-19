//! Coalesced local-save notifications, independent of the ambient sound bed.
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct Gate {
    observed: Option<(PathBuf, u64)>,
    pending: bool,
    played_at: Option<f64>,
}

impl Gate {
    pub fn update(
        &mut self,
        root: &Path,
        generation: u64,
        enabled: bool,
        speaking: bool,
        now: f64,
    ) -> bool {
        let Some((previous_root, previous_generation)) = self.observed.as_mut() else {
            self.observed = Some((root.to_owned(), generation));
            return false;
        };
        if previous_root != root || generation < *previous_generation {
            *previous_root = root.to_owned();
            *previous_generation = generation;
            self.pending = false;
            self.played_at = None;
            return false;
        }
        if generation != *previous_generation {
            *previous_generation = generation;
            self.pending |= enabled;
        }
        if !enabled {
            self.pending = false;
            return false;
        }
        if self.pending && !speaking && self.played_at.is_none_or(|last| now - last >= 0.75) {
            self.pending = false;
            self.played_at = Some(now);
            return true;
        }
        false
    }
}

const RATE: f32 = 44_100.;
const NOTE_FRAMES: u32 = 7_938;

pub(super) struct Tone {
    position: u32,
    gain: f32,
}

impl Default for Tone {
    fn default() -> Self {
        Self {
            position: NOTE_FRAMES * 2,
            gain: 0.,
        }
    }
}

impl Tone {
    pub fn trigger(&mut self) {
        self.position = 0;
    }

    pub fn frame(&mut self, volume: f32) -> f32 {
        self.gain += (volume.clamp(0., 1.) - self.gain) * 0.002;
        if self.position >= NOTE_FRAMES * 2 {
            return 0.;
        }
        let note = self.position / NOTE_FRAMES;
        let fraction = (self.position % NOTE_FRAMES) as f32 / NOTE_FRAMES as f32;
        let frequency = if note == 0 { 660. } else { 880. };
        let phase = std::f32::consts::TAU * frequency * fraction * NOTE_FRAMES as f32 / RATE;
        let envelope = (std::f32::consts::PI * fraction).sin().powi(2);
        self.position += 1;
        (phase.sin() + (phase * 2.).sin() * 0.12) * envelope * self.gain * 0.14
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given an observed worktree, a save batch yields one cue after narration,
    // while initial snapshots, muted saves and switching repositories stay quiet.
    #[test]
    fn local_saves_chime_once_without_interrupting_speech_or_replaying_muted_saves() {
        let mut gate = Gate::default();
        let root = Path::new("/fixture");
        assert!(!gate.update(root, 0, true, false, 0.));
        assert!(!gate.update(root, 1, true, true, 1.));
        assert!(!gate.update(root, 2, true, true, 1.1));
        assert!(gate.update(root, 2, true, false, 1.2));
        assert!(!gate.update(root, 2, true, false, 1.3));
        assert!(!gate.update(root, 3, false, false, 2.));
        assert!(!gate.update(root, 3, true, false, 3.));
        assert!(!gate.update(Path::new("/another"), 7, true, false, 4.));
        assert!(gate.update(Path::new("/another"), 8, true, false, 5.));
    }

    #[test]
    fn rapid_batches_are_coalesced_and_muting_discards_a_pending_cue() {
        let mut gate = Gate::default();
        let root = Path::new("/fixture");
        gate.update(root, 0, true, false, 0.);
        assert!(gate.update(root, 1, true, false, 1.));
        assert!(!gate.update(root, 2, true, false, 1.1));
        assert!(!gate.update(root, 3, true, false, 1.2));
        assert!(gate.update(root, 3, true, false, 1.8));
        assert!(!gate.update(root, 4, true, true, 3.));
        assert!(!gate.update(root, 4, false, false, 3.1));
        assert!(!gate.update(root, 4, true, false, 4.));
    }

    #[test]
    fn change_chime_is_short_bounded_and_fades_without_clicks() {
        let mut tone = Tone::default();
        assert_eq!(tone.frame(1.), 0.);
        tone.trigger();
        let mut previous = 0_f32;
        let mut energy = 0_f32;
        for frame in 0..44_100 {
            let sample = tone.frame(if frame < 9_000 { 1. } else { 0. });
            assert!(sample.is_finite() && sample.abs() < 0.16);
            assert!((sample - previous).abs() < 0.025);
            if frame >= NOTE_FRAMES * 2 {
                assert_eq!(sample, 0.);
            }
            energy += sample * sample;
            previous = sample;
        }
        assert!(energy > 10.);
        let mut muted = Tone::default();
        muted.trigger();
        assert!((0..44_100).all(|_| muted.frame(0.) == 0.));
    }
}
