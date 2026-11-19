//! Opt-in native PCM transport. Device callbacks use bounded SPSC sample queues.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};

const RATE: u32 = 24_000;
const CAPACITY: usize = RATE as usize * 4;
const PLAYBACK_CAPACITY: usize = RATE as usize * 30;

struct Ring {
    values: Box<[AtomicU32]>,
    read: AtomicUsize,
    write: AtomicUsize,
    overflow: AtomicBool,
}

impl Ring {
    fn new() -> Arc<Self> {
        Self::with_capacity(CAPACITY)
    }
    fn with_capacity(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            values: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
        })
    }
    fn push(&self, sample: f32) {
        let write = self.write.load(Ordering::Relaxed);
        let next = (write + 1) % self.values.len();
        if next == self.read.load(Ordering::Acquire) {
            self.overflow.store(true, Ordering::Relaxed);
            return;
        }
        self.values[write].store(sample.to_bits(), Ordering::Relaxed);
        self.write.store(next, Ordering::Release);
    }
    fn pop(&self) -> Option<f32> {
        let read = self.read.load(Ordering::Relaxed);
        if read == self.write.load(Ordering::Acquire) {
            return None;
        }
        let sample = f32::from_bits(self.values[read].load(Ordering::Relaxed));
        self.read
            .store((read + 1) % self.values.len(), Ordering::Release);
        Some(sample)
    }
}

pub(super) struct Devices {
    _input: cpal::Stream,
    _output: cpal::Stream,
    capture: Arc<Ring>,
    playback: Arc<Ring>,
    failed: Arc<AtomicBool>,
    captured: Vec<i16>,
    previous_output: f32,
    speaking: Arc<AtomicBool>,
}

impl Devices {
    /// Called only after an explicit microphone action and provider negotiation.
    pub fn open(speaking: Arc<AtomicBool>) -> Result<Self, String> {
        let host = cpal::default_host();
        let input = host
            .default_input_device()
            .ok_or("No microphone is available.")?;
        let output = host
            .default_output_device()
            .ok_or("No speaker is available.")?;
        let input_config = input
            .default_input_config()
            .map_err(|error| format!("Microphone configuration: {error}"))?;
        let output_config = output
            .default_output_config()
            .map_err(|error| format!("Speaker configuration: {error}"))?;
        let capture = Ring::new();
        let playback = Ring::with_capacity(PLAYBACK_CAPACITY);
        let failed = Arc::new(AtomicBool::new(false));
        let input_config_value: cpal::StreamConfig = input_config.clone().into();
        let output_config_value: cpal::StreamConfig = output_config.clone().into();
        let input_stream = match input_config.sample_format() {
            cpal::SampleFormat::F32 => input_stream::<f32>(
                &input,
                &input_config_value,
                capture.clone(),
                failed.clone(),
                speaking.clone(),
            ),
            cpal::SampleFormat::I16 => input_stream::<i16>(
                &input,
                &input_config_value,
                capture.clone(),
                failed.clone(),
                speaking.clone(),
            ),
            cpal::SampleFormat::U16 => input_stream::<u16>(
                &input,
                &input_config_value,
                capture.clone(),
                failed.clone(),
                speaking.clone(),
            ),
            format => {
                return Err(format!(
                    "Microphone sample format {format} is not supported."
                ));
            }
        }
        .map_err(|error| format!("Could not open microphone: {error}"))?;
        let output_stream = match output_config.sample_format() {
            cpal::SampleFormat::F32 => output_stream::<f32>(
                &output,
                &output_config_value,
                playback.clone(),
                failed.clone(),
                speaking.clone(),
            ),
            cpal::SampleFormat::I16 => output_stream::<i16>(
                &output,
                &output_config_value,
                playback.clone(),
                failed.clone(),
                speaking.clone(),
            ),
            cpal::SampleFormat::U16 => output_stream::<u16>(
                &output,
                &output_config_value,
                playback.clone(),
                failed.clone(),
                speaking.clone(),
            ),
            format => return Err(format!("Speaker sample format {format} is not supported.")),
        }
        .map_err(|error| format!("Could not open speaker: {error}"))?;
        output_stream
            .play()
            .map_err(|error| format!("Could not start speaker: {error}"))?;
        input_stream
            .play()
            .map_err(|error| format!("Could not start microphone: {error}"))?;
        Ok(Self {
            _input: input_stream,
            _output: output_stream,
            capture,
            playback,
            failed,
            captured: Vec::with_capacity(480),
            previous_output: 0.,
            speaking,
        })
    }

    pub fn capture(&mut self) -> Option<Vec<i16>> {
        while self.captured.len() < 480 {
            let sample = self.capture.pop()?;
            self.captured
                .push((sample.clamp(-1., 1.) * i16::MAX as f32) as i16);
        }
        Some(std::mem::replace(
            &mut self.captured,
            Vec::with_capacity(480),
        ))
    }

    /// PCMU supplies 8 kHz mono. Interpolate into the 24 kHz device queue.
    pub fn append(&mut self, samples: &[i16]) {
        for sample in samples {
            let next = f32::from(*sample) / 32768.;
            for step in 1..=3 {
                self.playback
                    .push(self.previous_output + (next - self.previous_output) * step as f32 / 3.);
            }
            self.previous_output = next;
        }
    }

    pub fn healthy(&self) -> bool {
        !self.failed.load(Ordering::Relaxed)
            && !self.capture.overflow.load(Ordering::Relaxed)
            && !self.playback.overflow.load(Ordering::Relaxed)
    }
}

impl Drop for Devices {
    fn drop(&mut self) {
        self.speaking.store(false, Ordering::Relaxed);
    }
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    ring: Arc<Ring>,
    failed: Arc<AtomicBool>,
    speaking: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.max(1);
    let mut phase = 0_u32;
    // Half duplex prevents speaker echo without inventing an untested native AEC.
    device.build_input_stream(
        config,
        move |samples: &[T], _| {
            for frame in samples.chunks_exact(channels) {
                let sample = if speaking.load(Ordering::Relaxed) {
                    0.
                } else {
                    frame
                        .iter()
                        .map(|sample| <f32 as cpal::FromSample<T>>::from_sample_(*sample))
                        .sum::<f32>()
                        / channels as f32
                };
                phase += RATE;
                while phase >= rate {
                    phase -= rate;
                    ring.push(sample);
                }
            }
        },
        move |_| {
            failed.store(true, Ordering::Relaxed);
        },
        None,
    )
}

fn output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    ring: Arc<Ring>,
    failed: Arc<AtomicBool>,
    speaking: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.max(1);
    let mut phase = rate;
    let mut current = 0_f32;
    let mut previous = 0_f32;
    let mut silence = RATE;
    device.build_output_stream(
        config,
        move |samples: &mut [T], _| {
            for frame in samples.chunks_exact_mut(channels) {
                while phase >= rate {
                    phase -= rate;
                    previous = current;
                    if let Some(next) = ring.pop() {
                        current = next;
                        silence = 0;
                    } else {
                        current = 0.;
                        silence = silence.saturating_add(1);
                    }
                }
                let sample = previous + (current - previous) * phase as f32 / rate as f32;
                frame.fill(T::from_sample_(sample));
                phase += RATE;
            }
            speaking.store(silence < RATE / 5, Ordering::Relaxed);
        },
        move |_| {
            failed.store(true, Ordering::Relaxed);
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_is_bounded_and_preserves_order_without_overwriting_unread_audio() {
        let queue = Ring::new();
        for sample in 0..CAPACITY {
            queue.push(sample as f32);
        }
        assert!(queue.overflow.load(Ordering::Relaxed));
        for sample in 0..CAPACITY - 1 {
            assert_eq!(queue.pop(), Some(sample as f32));
        }
        assert_eq!(queue.pop(), None);
    }
}
