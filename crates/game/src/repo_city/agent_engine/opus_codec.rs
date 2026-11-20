//! Wideband native voice codec. libopus is bundled and statically linked.
use opus::{Application, Bitrate, Channels};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub(crate) const SAMPLE_RATE: u32 = 48_000;
pub(crate) const FRAME_SAMPLES: usize = 960;
const MAX_SAMPLES: usize = 5760; // Opus permits up to 120 ms in one packet.
const MAX_PACKET: usize = 1275;
const REORDER_DELAY: Duration = Duration::from_millis(40);
const MAX_PENDING: usize = 16;

pub(crate) struct Encoder(opus::Encoder);
impl Encoder {
    pub fn new() -> Result<Self, String> {
        let mut encoder = opus::Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)
            .map_err(codec_error)?;
        encoder
            .set_bitrate(Bitrate::Bits(64_000))
            .map_err(codec_error)?;
        encoder.set_complexity(9).map_err(codec_error)?;
        encoder.set_inband_fec(true).map_err(codec_error)?;
        encoder.set_packet_loss_perc(10).map_err(codec_error)?;
        encoder.set_dtx(true).map_err(codec_error)?;
        Ok(Self(encoder))
    }
    pub fn encode(&mut self, input: &[i16]) -> Result<Vec<u8>, String> {
        if input.len() != FRAME_SAMPLES {
            return Err("Native Opus requires 960 mono samples per 20 ms frame.".into());
        }
        self.0.encode_vec(input, MAX_PACKET).map_err(codec_error)
    }
}

pub(super) struct Decoder(opus::Decoder);
impl Decoder {
    pub fn new() -> Result<Self, String> {
        // libopus downmixes stereo packets when the output decoder is mono.
        opus::Decoder::new(SAMPLE_RATE, Channels::Mono)
            .map(Self)
            .map_err(codec_error)
    }
    pub fn decode(&mut self, packet: &[u8]) -> Result<Vec<i16>, String> {
        if packet.is_empty() || packet.len() > MAX_PACKET {
            return Err("Invalid native Opus packet size.".into());
        }
        let mut output = vec![0; MAX_SAMPLES];
        let samples = self
            .0
            .decode(packet, &mut output, false)
            .map_err(codec_error)?;
        output.truncate(samples);
        Ok(output)
    }
    pub fn conceal(&mut self, samples: usize) -> Result<Vec<i16>, String> {
        self.recover(&[], samples)
    }
    fn recover(&mut self, following: &[u8], samples: usize) -> Result<Vec<i16>, String> {
        let mut output = vec![0; samples.clamp(120, MAX_SAMPLES)];
        let samples = self
            .0
            .decode(following, &mut output, !following.is_empty())
            .map_err(codec_error)?;
        output.truncate(samples);
        Ok(output)
    }
}
fn codec_error(error: opus::Error) -> String {
    format!("Native Opus: {error}")
}

struct Packet {
    sequence: u16,
    arrived: Instant,
    payload: Vec<u8>,
}
/// A small reorder window, followed by Opus FEC/PLC. Never fabricates unbounded
/// audio across DTX gaps or reconnects; RTP timestamps are owned by the sender.
pub(crate) struct Playout {
    decoder: Decoder,
    pending: VecDeque<Packet>,
    expected: Option<u16>,
    frame_samples: usize,
    pub concealed: u64,
    pub dropped: u64,
    pub invalid: u64,
}
impl Playout {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            decoder: Decoder::new()?,
            pending: VecDeque::new(),
            expected: None,
            frame_samples: FRAME_SAMPLES,
            concealed: 0,
            dropped: 0,
            invalid: 0,
        })
    }
    pub fn push(&mut self, sequence: u16, payload: Vec<u8>, now: Instant) {
        if payload.is_empty() || payload.len() > MAX_PACKET {
            self.invalid += 1;
            return;
        }
        let expected = *self.expected.get_or_insert(sequence);
        if sequence.wrapping_sub(expected) > 32767
            || self.pending.iter().any(|p| p.sequence == sequence)
        {
            self.dropped += 1;
            return;
        }
        if self.pending.len() == MAX_PENDING {
            // Network bursts may overrun playback; dropping audio is preferable to
            // terminating the call. Keep the next frames nearest the play position.
            self.dropped += 1;
            return;
        }
        let position = self
            .pending
            .iter()
            .position(|p| p.sequence.wrapping_sub(expected) > sequence.wrapping_sub(expected))
            .unwrap_or(self.pending.len());
        self.pending.insert(
            position,
            Packet {
                sequence,
                arrived: now,
                payload,
            },
        );
    }
    pub fn drain(&mut self, now: Instant) -> Vec<Vec<i16>> {
        let mut output = Vec::new();
        while self
            .pending
            .front()
            .is_some_and(|p| now.saturating_duration_since(p.arrived) >= REORDER_DELAY)
        {
            let packet = self.pending.pop_front().unwrap();
            let gap = packet
                .sequence
                .wrapping_sub(self.expected.unwrap_or(packet.sequence));
            // A large gap is a discontinuity, not minutes of synthesized audio.
            if gap <= 3 {
                for missing in 0..gap {
                    let following = if missing + 1 == gap {
                        packet.payload.as_slice()
                    } else {
                        &[]
                    };
                    if let Ok(samples) = self.decoder.recover(following, self.frame_samples) {
                        output.push(samples);
                    }
                    self.concealed += 1;
                }
            }
            match self.decoder.decode(&packet.payload) {
                Ok(samples) => {
                    self.frame_samples = samples.len();
                    output.push(samples);
                }
                Err(_) => {
                    self.invalid += 1;
                    if let Ok(samples) = self.decoder.conceal(self.frame_samples) {
                        output.push(samples);
                    }
                    self.concealed += 1;
                }
            }
            self.expected = Some(packet.sequence.wrapping_add(1));
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(offset: usize) -> Vec<i16> {
        (offset..offset + FRAME_SAMPLES)
            .map(|i| {
                let time = i as f64 / SAMPLE_RATE as f64;
                (7500. * (time * 1000. * std::f64::consts::TAU).sin()
                    + 7500. * (time * 6500. * std::f64::consts::TAU).sin()) as i16
            })
            .collect()
    }

    #[test]
    fn opus_preserves_voice_and_high_frequencies_in_real_frames() {
        let mut encoder = Encoder::new().unwrap();
        let mut decoder = Decoder::new().unwrap();
        let mut original = Vec::new();
        let mut decoded = Vec::new();
        for frame in 0..30 {
            let input = signal(frame * FRAME_SAMPLES);
            let packet = encoder.encode(&input).unwrap();
            assert!(packet.len() < 1276);
            original.extend(input);
            decoded.extend(decoder.decode(&packet).unwrap());
        }
        // Opus has algorithmic lookahead; compare after alignment, excluding startup.
        let delay = (0..600)
            .max_by(|a, b| {
                let score = |lag: usize| {
                    original[2000..10000]
                        .iter()
                        .zip(&decoded[2000 + lag..10000 + lag])
                        .map(|(a, b)| *a as f64 * *b as f64)
                        .sum::<f64>()
                };
                score(*a).total_cmp(&score(*b))
            })
            .unwrap();
        let energy = original[2000..20000]
            .iter()
            .map(|x| (*x as f64).powi(2))
            .sum::<f64>();
        let error = original[2000..20000]
            .iter()
            .zip(&decoded[2000 + delay..20000 + delay])
            .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
            .sum::<f64>();
        let snr = 10. * (energy / error).log10();
        assert!(snr > 14., "Opus SNR {snr:.1} dB, delay {delay}");
    }

    #[test]
    fn lost_and_corrupted_packets_do_not_poison_decoder() {
        let mut encoder = Encoder::new().unwrap();
        let mut decoder = Decoder::new().unwrap();
        let packet = encoder.encode(&signal(0)).unwrap();
        assert_eq!(decoder.decode(&packet).unwrap().len(), FRAME_SAMPLES);
        assert_eq!(decoder.conceal(FRAME_SAMPLES).unwrap().len(), FRAME_SAMPLES);
        assert!(decoder.decode(&[255; 1500]).is_err());
        assert_eq!(decoder.decode(&packet).unwrap().len(), FRAME_SAMPLES);
    }
    #[test]
    fn playout_reorders_wraps_conceals_and_accepts_dtx_without_ending_call() {
        let mut encoder = Encoder::new().unwrap();
        let mut playout = Playout::new().unwrap();
        let start = Instant::now();
        let packet = encoder.encode(&signal(0)).unwrap();
        playout.push(65534, packet.clone(), start);
        playout.push(0, packet.clone(), start); // 65535 arrives out of order.
        playout.push(65535, packet.clone(), start);
        assert!(playout.drain(start).is_empty());
        assert_eq!(playout.drain(start + REORDER_DELAY).len(), 3);
        playout.push(2, packet.clone(), start + REORDER_DELAY); // packet 1 lost.
        assert_eq!(playout.drain(start + REORDER_DELAY * 2).len(), 2);
        assert_eq!(playout.concealed, 1);
        playout.push(3, packet, start + Duration::from_secs(4)); // DTX silence changes time, not sequence.
        assert_eq!(playout.drain(start + Duration::from_secs(5)).len(), 1);
        assert_eq!(playout.concealed, 1);
    }
    #[test]
    fn playout_bounds_bursts_and_large_loss_without_failing() {
        let mut encoder = Encoder::new().unwrap();
        let mut playout = Playout::new().unwrap();
        let now = Instant::now();
        let packet = encoder.encode(&signal(0)).unwrap();
        for sequence in 0..1000 {
            playout.push(sequence, packet.clone(), now);
        }
        assert_eq!(playout.pending.len(), MAX_PENDING);
        assert_eq!(playout.drain(now + REORDER_DELAY).len(), MAX_PENDING);
        playout.push(1001, packet, now);
        assert_eq!(playout.drain(now + REORDER_DELAY).len(), 1);
        assert!(playout.dropped > 0);
    }
}
