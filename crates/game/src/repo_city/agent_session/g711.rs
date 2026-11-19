//! ITU-T G.711 mu-law: the negotiated PCMU payload, with no native codec library.
pub(super) fn encode(samples: &[i16]) -> Vec<u8> {
    samples
        .chunks_exact(3)
        .map(|samples| {
            let value = samples.iter().map(|sample| i32::from(*sample)).sum::<i32>() / 3;
            let sign = if value < 0 { 0x80 } else { 0 };
            let magnitude = value.abs().min(32635) + 132;
            let mut exponent = 7;
            let mut mask = 0x4000;
            while exponent > 0 && magnitude & mask == 0 {
                exponent -= 1;
                mask >>= 1;
            }
            let mantissa = (magnitude >> (exponent + 3)) & 0x0f;
            !(sign | ((exponent << 4) as u8) | mantissa as u8)
        })
        .collect()
}

pub(super) fn decode(bytes: &[u8]) -> Vec<i16> {
    bytes
        .iter()
        .map(|byte| {
            let value = !*byte;
            let magnitude = ((i32::from(value & 0x0f) << 3) + 132) << ((value >> 4) & 0x07);
            (if value & 0x80 == 0 {
                magnitude - 132
            } else {
                132 - magnitude
            }) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcmu_matches_standard_zero_polarity_and_full_scale_vectors() {
        assert_eq!(
            encode(&[
                0, 0, 0, 1000, 1000, 1000, -1000, -1000, -1000, 32767, 32767, 32767, -32768,
                -32768, -32768
            ]),
            vec![0xff, 0xce, 0x4e, 0x80, 0x00]
        );
        assert_eq!(
            decode(&[0xff, 0x7f, 0xce, 0x4e, 0x80, 0x00]),
            vec![0, 0, 988, -988, 32124, -32124]
        );
    }
    #[test]
    fn twenty_ms_audio_has_exact_packet_duration_and_bounded_quantization_error() {
        assert_eq!(encode(&[0; 480]).len(), 160);
        for sample in i16::MIN..=i16::MAX {
            let decoded = decode(&encode(&[sample; 3]))[0];
            assert!((i32::from(sample) - i32::from(decoded)).abs() <= 644);
        }
    }
}
