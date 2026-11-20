//! Opt-in native PCM transport. Device callbacks use bounded SPSC sample queues.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};
use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const RATE: u32 = super::opus_codec::SAMPLE_RATE;
const FRAME_SAMPLES: usize = super::opus_codec::FRAME_SAMPLES;
const PREBUFFER: usize = RATE as usize * 60 / 1000;
const CAPACITY: usize = RATE as usize * 4;
const PLAYBACK_CAPACITY: usize = RATE as usize * 30;

#[derive(Default)]
struct Metrics {
    input_frames: AtomicU64,
    input_peak: AtomicU32,
    captured_peak: AtomicU32,
    gated_frames: AtomicU64,
    output_frames: AtomicU64,
    output_peak: AtomicU32,
    input_rate: AtomicU32,
    output_rate: AtomicU32,
    underruns: AtomicU64,
}

static DEVICE_OPENING: AtomicBool = AtomicBool::new(false);
struct OpeningPermit;
impl OpeningPermit {
    fn acquire() -> Option<Self> {
        DEVICE_OPENING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}
impl Drop for OpeningPermit {
    fn drop(&mut self) {
        DEVICE_OPENING.store(false, Ordering::Release);
    }
}

pub(crate) struct Opening {
    result: Receiver<Result<Devices, String>>,
    active: Arc<AtomicBool>,
    started: Instant,
    delivered: bool,
}
impl Opening {
    pub fn start(speaking: Arc<AtomicBool>) -> Self {
        let (send, result) = mpsc::sync_channel(1);
        let active = Arc::new(AtomicBool::new(true));
        let alive = active.clone();
        if let Some(permit) = OpeningPermit::acquire() {
            std::thread::spawn(move || {
                let _permit = permit;
                let _ = send.send(Devices::open(speaking, alive));
            });
        } else {
            let _ = send.try_send(Err("Audio devices are still opening. Finish the macOS permission prompt before starting voice again.".into()));
        }
        Self {
            result,
            active,
            started: Instant::now(),
            delivered: false,
        }
    }
    pub fn poll(&mut self) -> Option<Result<Devices, String>> {
        match self.result.try_recv() {
            Ok(result) => {
                self.delivered = result.is_ok();
                Some(result)
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Native audio opening stopped unexpectedly.".into()))
            }
            Err(mpsc::TryRecvError::Empty) if self.started.elapsed() >= Duration::from_secs(12) => {
                self.active.store(false, Ordering::Release);
                Some(Err("Microphone or speaker did not open within 12 seconds. Check microphone access for Repo City in System Settings, then start voice again.".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
}
impl Drop for Opening {
    fn drop(&mut self) {
        if !self.delivered {
            self.active.store(false, Ordering::Release);
        }
    }
}

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
    fn available(&self) -> usize {
        let read = self.read.load(Ordering::Acquire);
        let write = self.write.load(Ordering::Acquire);
        (write + self.values.len() - read) % self.values.len()
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

pub(crate) struct Devices {
    _input: cpal::Stream,
    _output: cpal::Stream,
    capture: Arc<Ring>,
    playback: Arc<Ring>,
    failed: Arc<AtomicBool>,
    captured: Vec<i16>,
    speaking: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    metrics: Arc<Metrics>,
}

impl Devices {
    /// Called only after an explicit microphone action and provider negotiation.
    fn open(speaking: Arc<AtomicBool>, active: Arc<AtomicBool>) -> Result<Self, String> {
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
        let metrics = Arc::new(Metrics::default());
        let input_config_value: cpal::StreamConfig = input_config.clone().into();
        let output_config_value: cpal::StreamConfig = output_config.clone().into();
        let input_stream = match input_config.sample_format() {
            cpal::SampleFormat::F32 => input_stream::<f32>(
                &input,
                &input_config_value,
                capture.clone(),
                failed.clone(),
                speaking.clone(),
                active.clone(),
                metrics.clone(),
            ),
            cpal::SampleFormat::I16 => input_stream::<i16>(
                &input,
                &input_config_value,
                capture.clone(),
                failed.clone(),
                speaking.clone(),
                active.clone(),
                metrics.clone(),
            ),
            cpal::SampleFormat::U16 => input_stream::<u16>(
                &input,
                &input_config_value,
                capture.clone(),
                failed.clone(),
                speaking.clone(),
                active.clone(),
                metrics.clone(),
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
                active.clone(),
                metrics.clone(),
            ),
            cpal::SampleFormat::I16 => output_stream::<i16>(
                &output,
                &output_config_value,
                playback.clone(),
                failed.clone(),
                speaking.clone(),
                active.clone(),
                metrics.clone(),
            ),
            cpal::SampleFormat::U16 => output_stream::<u16>(
                &output,
                &output_config_value,
                playback.clone(),
                failed.clone(),
                speaking.clone(),
                active.clone(),
                metrics.clone(),
            ),
            format => return Err(format!("Speaker sample format {format} is not supported.")),
        }
        .map_err(|error| format!("Could not open speaker: {error}"))?;
        if !active.load(Ordering::Acquire) {
            return Err("Audio opening canceled.".into());
        }
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
            captured: Vec::with_capacity(FRAME_SAMPLES),
            speaking,
            active,
            metrics,
        })
    }

    pub fn capture(&mut self) -> Option<Vec<i16>> {
        while self.captured.len() < FRAME_SAMPLES {
            let sample = self.capture.pop()?;
            self.captured
                .push((sample.clamp(-1., 1.) * i16::MAX as f32) as i16);
        }
        Some(std::mem::replace(
            &mut self.captured,
            Vec::with_capacity(FRAME_SAMPLES),
        ))
    }

    /// Opus supplies 48 kHz mono, matching the bounded playback queue.
    pub fn append(&mut self, samples: &[i16]) {
        for sample in samples {
            self.playback.push(f32::from(*sample) / 32768.);
        }
    }

    pub fn diagnostics(&self) -> Value {
        json!({"codec_rate":RATE,"frame_samples":FRAME_SAMPLES,"input_device_rate":self.metrics.input_rate.load(Ordering::Relaxed),"output_device_rate":self.metrics.output_rate.load(Ordering::Relaxed),"playback_queued_samples":self.playback.available(),"playback_underruns":self.metrics.underruns.load(Ordering::Relaxed),"input_frames":self.metrics.input_frames.load(Ordering::Relaxed),"input_peak":f32::from_bits(self.metrics.input_peak.swap(0,Ordering::Relaxed)),"captured_peak":f32::from_bits(self.metrics.captured_peak.swap(0,Ordering::Relaxed)),"gated_frames":self.metrics.gated_frames.load(Ordering::Relaxed),"output_frames":self.metrics.output_frames.load(Ordering::Relaxed),"output_peak":f32::from_bits(self.metrics.output_peak.swap(0,Ordering::Relaxed)),"capture_overflow":self.capture.overflow.load(Ordering::Relaxed),"playback_overflow":self.playback.overflow.load(Ordering::Relaxed),"speaking":self.speaking.load(Ordering::Relaxed)})
    }

    pub fn healthy(&self) -> bool {
        !self.failed.load(Ordering::Relaxed)
            && !self.capture.overflow.load(Ordering::Relaxed)
            && !self.playback.overflow.load(Ordering::Relaxed)
    }
}

impl Drop for Devices {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        self.speaking.store(false, Ordering::Relaxed);
    }
}

struct Playback {
    phase: u32,
    current: f32,
    previous: f32,
    silence: u32,
    primed: bool,
    waiting: u32,
    underruns: u64,
}

impl Playback {
    fn new(rate: u32) -> Self {
        Self {
            phase: rate,
            current: 0.,
            previous: 0.,
            silence: RATE,
            primed: false,
            waiting: 0,
            underruns: 0,
        }
    }
    fn frame(&mut self, ring: &Ring, rate: u32) -> f32 {
        if !self.primed {
            if ring.available() >= PREBUFFER
                || (ring.available() > 0 && self.waiting >= rate * 60 / 1000)
            {
                self.primed = true;
                self.waiting = 0;
            } else {
                self.waiting = if ring.available() > 0 {
                    self.waiting.saturating_add(1)
                } else {
                    0
                };
                self.phase = self.phase.saturating_add(RATE);
                self.silence = self.silence.saturating_add(self.phase / rate);
                self.phase %= rate;
                self.previous = 0.;
                self.current = 0.;
                return 0.;
            }
        }
        while self.phase >= rate {
            self.phase -= rate;
            self.previous = self.current;
            self.current = if let Some(sample) = ring.pop() {
                sample
            } else {
                self.primed = false;
                self.waiting = 0;
                self.underruns += 1;
                0.
            };
            // RTP can contain an uninterrupted stream of digital silence. Only
            // audible output should suspend capture, never packet availability.
            if self.current.abs() > 0.004 {
                self.silence = 0;
            } else {
                self.silence = self.silence.saturating_add(1);
            }
        }
        let sample =
            self.previous + (self.current - self.previous) * self.phase as f32 / rate as f32;
        self.phase += RATE;
        sample
    }
    fn speaking(&self) -> bool {
        self.silence < RATE / 5
    }
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    ring: Arc<Ring>,
    failed: Arc<AtomicBool>,
    speaking: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    metrics: Arc<Metrics>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.max(1);
    let mut phase = 0_u32;
    let mut previous = 0_f32;
    metrics.input_rate.store(rate, Ordering::Relaxed);
    // Half duplex prevents speaker echo without inventing an untested native AEC.
    device.build_input_stream(
        config,
        move |samples: &[T], _| {
            if !active.load(Ordering::Acquire) {
                return;
            }
            let mut peak = 0_f32;
            let mut captured_peak = 0_f32;
            let mut gated = 0_u64;
            for frame in samples.chunks_exact(channels) {
                let raw = frame
                    .iter()
                    .map(|sample| <f32 as cpal::FromSample<T>>::from_sample_(*sample))
                    .sum::<f32>()
                    / channels as f32;
                peak = peak.max(raw.abs());
                let sample = if speaking.load(Ordering::Relaxed) {
                    gated += 1;
                    0.
                } else {
                    raw
                };
                captured_peak = captured_peak.max(sample.abs());
                phase += RATE;
                while phase >= rate {
                    phase -= rate;
                    let fraction = 1. - phase as f32 / RATE as f32;
                    ring.push(previous + (sample - previous) * fraction.clamp(0., 1.));
                }
                previous = sample;
            }
            metrics
                .input_frames
                .fetch_add((samples.len() / channels) as u64, Ordering::Relaxed);
            metrics.gated_frames.fetch_add(gated, Ordering::Relaxed);
            metrics
                .input_peak
                .fetch_max(peak.to_bits(), Ordering::Relaxed);
            metrics
                .captured_peak
                .fetch_max(captured_peak.to_bits(), Ordering::Relaxed);
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
    active: Arc<AtomicBool>,
    metrics: Arc<Metrics>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.max(1);
    let mut playback = Playback::new(rate);
    metrics.output_rate.store(rate, Ordering::Relaxed);
    device.build_output_stream(
        config,
        move |samples: &mut [T], _| {
            if !active.load(Ordering::Acquire) {
                samples.fill(T::from_sample_(0.));
                return;
            }
            let mut peak = 0_f32;
            for frame in samples.chunks_exact_mut(channels) {
                let sample = playback.frame(&ring, rate);
                peak = peak.max(sample.abs());
                frame.fill(T::from_sample_(sample));
            }
            metrics
                .output_frames
                .fetch_add((samples.len() / channels) as u64, Ordering::Relaxed);
            metrics
                .output_peak
                .fetch_max(peak.to_bits(), Ordering::Relaxed);
            metrics
                .underruns
                .store(playback.underruns, Ordering::Relaxed);
            speaking.store(playback.speaking(), Ordering::Relaxed);
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
    fn short_network_bursts_are_buffered_and_short_tails_eventually_play() {
        let queue = Ring::new();
        for _ in 0..FRAME_SAMPLES {
            queue.push(0.2);
        }
        let mut playback = Playback::new(RATE);
        for _ in 0..RATE * 30 / 1000 {
            assert_eq!(playback.frame(&queue, RATE), 0.);
        }
        for _ in 0..FRAME_SAMPLES {
            queue.push(0.2);
        }
        for _ in 0..RATE * 30 / 1000 {
            assert_eq!(playback.frame(&queue, RATE), 0.);
        }
        let played = (0..FRAME_SAMPLES * 2)
            .filter(|_| playback.frame(&queue, RATE) > 0.1)
            .count();
        assert!(
            played >= FRAME_SAMPLES * 2 - 2,
            "buffered speech must not contain network gaps"
        );
        for _ in 0..RATE / 3 {
            playback.frame(&queue, RATE);
        }
        assert!(!playback.speaking());
    }

    #[test]
    fn a_blocked_device_open_prevents_another_worker_and_releases_its_permit() {
        let permit = OpeningPermit::acquire().expect("no audio device is opened in unit tests");
        let mut opening = Opening::start(Arc::new(AtomicBool::new(false)));
        assert!(matches!(opening.poll(), Some(Err(error)) if error.contains("still opening")));
        drop(opening);
        assert!(OpeningPermit::acquire().is_none());
        drop(permit);
        assert!(OpeningPermit::acquire().is_some());
    }

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

    // Given continuous silent RTP, playing queued packets must leave the mic open.
    #[test]
    fn continuous_silent_remote_packets_do_not_mute_the_microphone() {
        for silence in [0., 8. / 32768.] {
            let queue = Ring::new();
            for _ in 0..RATE * 3 {
                queue.push(silence);
            }
            let mut playback = Playback::new(48000);
            for _ in 0..48000 * 3 {
                playback.frame(&queue, 48000);
                assert!(
                    !playback.speaking(),
                    "silent RTP was mistaken for an active speaker"
                );
            }
        }
    }

    #[test]
    fn audible_playback_mutes_mic_then_releases_during_streamed_silence() {
        let queue = Ring::new();
        for _ in 0..RATE / 10 {
            queue.push(0.2);
        }
        for _ in 0..RATE {
            queue.push(0.);
        }
        let mut playback = Playback::new(48000);
        for _ in 0..4800 {
            playback.frame(&queue, 48000);
        }
        assert!(playback.speaking());
        for _ in 0..48000 * 199 / 1000 {
            playback.frame(&queue, 48000);
        }
        assert!(playback.speaking());
        for _ in 0..480 {
            playback.frame(&queue, 48000);
        }
        assert!(!playback.speaking());
    }

    #[test]
    fn blocked_audio_open_times_out_and_disarms_late_capture_without_touching_devices() {
        let (_send, result) = mpsc::sync_channel(1);
        let active = Arc::new(AtomicBool::new(true));
        let mut opening = Opening {
            result,
            active: active.clone(),
            started: Instant::now() - Duration::from_secs(13),
            delivered: false,
        };
        assert!(matches!(opening.poll(), Some(Err(_))));
        assert!(!active.load(Ordering::Acquire));
    }

    #[test]
    fn canceling_pending_audio_open_disarms_capture_before_any_result_arrives() {
        let (_send, result) = mpsc::sync_channel(1);
        let active = Arc::new(AtomicBool::new(true));
        let opening = Opening {
            result,
            active: active.clone(),
            started: Instant::now(),
            delivered: false,
        };
        drop(opening);
        assert!(!active.load(Ordering::Acquire));
    }
}
