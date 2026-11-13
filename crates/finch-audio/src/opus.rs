//! Opus encoding and decoding wrapper using `audiopus`.

use audiopus::coder::{Decoder, Encoder};
use audiopus::packet::Packet;
use audiopus::{Application, Bitrate, Channels, MutSignals, SampleRate};
use bytes::Bytes;

use crate::resampler::FRAME_SIZE_48K;

pub const MAX_OPUS_PACKET_SIZE: usize = 1000;

#[derive(Debug, thiserror::Error)]
pub enum AudioOpusError {
    #[error("Opus error: {0}")]
    Opus(#[from] audiopus::Error),
    #[error("Signal conversion error")]
    SignalConversion,
}

/// Packet-loss percentage to give the Opus encoder for in-band FEC, from the loss measured on the
/// sender's connection: 1.5 times the measured loss, kept within 2..=30. Without samples (`sent == 0`)
/// the encoder keeps its default of 5.
pub fn loss_to_fec_percent(lost: u64, sent: u64) -> i32 {
    if sent == 0 {
        return 5;
    }
    let percent = (lost as f64 * 150.0 / sent as f64).round();
    percent.clamp(2.0, 30.0) as i32
}

pub struct OpusVoiceEncoder {
    encoder: Encoder,
    output_buf: [u8; MAX_OPUS_PACKET_SIZE],
}

impl OpusVoiceEncoder {
    pub fn new(bitrate_kbps: u32) -> Result<Self, AudioOpusError> {
        let mut encoder = Encoder::new(
            SampleRate::Hz48000,
            Channels::Mono,
            Application::Voip,
        )?;

        encoder.set_bitrate(Bitrate::BitsPerSecond((bitrate_kbps * 1000) as i32))?;
        encoder.set_inband_fec(true)?;
        encoder.set_packet_loss_perc(5)?;

        Ok(Self {
            encoder,
            output_buf: [0u8; MAX_OPUS_PACKET_SIZE],
        })
    }

    pub fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<(), AudioOpusError> {
        self.encoder.set_bitrate(Bitrate::BitsPerSecond((bitrate_kbps * 1000) as i32))?;
        Ok(())
    }

    /// Current target bitrate in bit/s. Only `BitsPerSecond` is ever set here; other modes report 0.
    pub fn bitrate_bps(&self) -> Result<i32, AudioOpusError> {
        Ok(match self.encoder.bitrate()? {
            Bitrate::BitsPerSecond(bps) => bps,
            _ => 0,
        })
    }

    pub fn set_packet_loss_perc(&mut self, loss_perc: u8) -> Result<(), AudioOpusError> {
        self.encoder.set_packet_loss_perc(loss_perc)?;
        Ok(())
    }

    /// Change the expected packet loss (percent) in place; the encoder keeps its state. Drives how much
    /// in-band FEC redundancy it emits.
    pub fn set_expected_loss(&mut self, percent: u8) -> Result<(), AudioOpusError> {
        self.set_packet_loss_perc(percent)
    }

    /// Expected packet loss (percent) the encoder currently assumes.
    pub fn packet_loss_perc(&self) -> Result<u8, AudioOpusError> {
        Ok(self.encoder.packet_loss_perc()?)
    }

    pub fn encode(&mut self, samples: &[f32; FRAME_SIZE_48K]) -> Result<Bytes, AudioOpusError> {
        let len = self.encoder.encode_float(&samples[..], &mut self.output_buf)?;
        Ok(Bytes::copy_from_slice(&self.output_buf[..len]))
    }
}

pub struct OpusVoiceDecoder {
    decoder: Decoder,
}

impl OpusVoiceDecoder {
    pub fn new() -> Result<Self, AudioOpusError> {
        let decoder = Decoder::new(SampleRate::Hz48000, Channels::Mono)?;
        Ok(Self { decoder })
    }

    pub fn decode(&mut self, payload: &[u8], output: &mut [f32; FRAME_SIZE_48K]) -> Result<usize, AudioOpusError> {
        let packet: Packet = payload.try_into().map_err(|_| AudioOpusError::SignalConversion)?;
        let signals: MutSignals<f32> = (&mut output[..]).try_into().map_err(|_| AudioOpusError::SignalConversion)?;
        let decoded = self.decoder.decode_float(Some(packet), signals, false)?;
        output[decoded..].fill(0.0);
        Ok(decoded)
    }

    /// Rebuild the frame that immediately precedes `next_payload` from the in-band FEC
    /// carried by the next packet. Call this only for a lost slot whose successor arrived.
    pub fn decode_fec(&mut self, next_payload: &[u8], output: &mut [f32; FRAME_SIZE_48K]) -> Result<usize, AudioOpusError> {
        let packet: Packet = next_payload.try_into().map_err(|_| AudioOpusError::SignalConversion)?;
        let signals: MutSignals<f32> = (&mut output[..]).try_into().map_err(|_| AudioOpusError::SignalConversion)?;
        let decoded = self.decoder.decode_float(Some(packet), signals, true)?;
        output[decoded..].fill(0.0);
        Ok(decoded)
    }

    pub fn decode_plc(&mut self, output: &mut [f32; FRAME_SIZE_48K]) -> Result<usize, AudioOpusError> {
        let signals: MutSignals<f32> = (&mut output[..]).try_into().map_err(|_| AudioOpusError::SignalConversion)?;
        let decoded = self.decoder.decode_float(None::<Packet>, signals, false)?;
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_is_applied_at_creation_and_changed_in_place() {
        let mut encoder = OpusVoiceEncoder::new(96).unwrap();
        assert_eq!(encoder.bitrate_bps().unwrap(), 96_000);
        encoder.set_bitrate(16).unwrap();
        assert_eq!(encoder.bitrate_bps().unwrap(), 16_000);
        encoder.set_bitrate(128).unwrap();
        assert_eq!(encoder.bitrate_bps().unwrap(), 128_000);
    }

    #[test]
    fn loss_to_fec_percent_scales_measured_loss_by_one_and_a_half_within_bounds() {
        assert_eq!(loss_to_fec_percent(0, 1000), 2);
        assert_eq!(loss_to_fec_percent(1, 100), 2);
        assert_eq!(loss_to_fec_percent(10, 100), 15);
        assert_eq!(loss_to_fec_percent(50, 100), 30);
        assert_eq!(loss_to_fec_percent(90, 100), 30);
    }

    #[test]
    fn loss_to_fec_percent_defaults_to_five_without_samples() {
        assert_eq!(loss_to_fec_percent(0, 0), 5);
        assert_eq!(loss_to_fec_percent(7, 0), 5);
    }

    #[test]
    fn expected_loss_is_applied_in_place_without_a_new_encoder() {
        let mut encoder = OpusVoiceEncoder::new(24).unwrap();
        assert_eq!(encoder.packet_loss_perc().unwrap(), 5);
        encoder.set_expected_loss(15).unwrap();
        assert_eq!(encoder.packet_loss_perc().unwrap(), 15);
        encoder.set_expected_loss(2).unwrap();
        assert_eq!(encoder.packet_loss_perc().unwrap(), 2);
        assert!(!encoder.encode(&[0.1f32; FRAME_SIZE_48K]).unwrap().is_empty());
    }

    #[test]
    fn shorter_packet_clears_unused_samples_in_reused_output() {
        let encoder = Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip).unwrap();
        let mut bytes = [0u8; MAX_OPUS_PACKET_SIZE];
        let length = encoder.encode_float(&[0.1; FRAME_SIZE_48K / 2], &mut bytes).unwrap();
        let mut decoder = OpusVoiceDecoder::new().unwrap();
        let mut output = [0.75; FRAME_SIZE_48K];
        let decoded = decoder.decode(&Bytes::copy_from_slice(&bytes[..length]), &mut output).unwrap();
        assert_eq!(decoded, FRAME_SIZE_48K / 2);
        assert!(output[decoded..].iter().all(|sample| *sample == 0.0),
            "a short packet must not replay samples left by the preceding frame");
    }

    #[test]
    fn test_opus_encode_decode_roundtrip() {
        let mut encoder = OpusVoiceEncoder::new(64).unwrap();
        let mut decoder = OpusVoiceDecoder::new().unwrap();

        let mut input = [0.0f32; FRAME_SIZE_48K];
        for (i, sample) in input.iter_mut().enumerate() {
            *sample = (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / 48000.0).sin() * 0.5;
        }

        let encoded = encoder.encode(&input).unwrap();
        assert!(!encoded.is_empty());
        assert!(encoded.len() < MAX_OPUS_PACKET_SIZE);

        let mut decoded = [0.0f32; FRAME_SIZE_48K];
        let samples_decoded = decoder.decode(&encoded, &mut decoded).unwrap();
        assert_eq!(samples_decoded, FRAME_SIZE_48K);

        // Test PLC
        let mut plc_out = [0.0f32; FRAME_SIZE_48K];
        let plc_samples = decoder.decode_plc(&mut plc_out).unwrap();
        assert_eq!(plc_samples, FRAME_SIZE_48K);
    }

    #[test]
    fn decode_fec_returns_one_frame_rebuilt_from_the_next_packet() {
        let mut encoder = OpusVoiceEncoder::new(24).unwrap();
        let mut decoder = OpusVoiceDecoder::new().unwrap();
        let packets: Vec<Bytes> = (0..8usize)
            .map(|k| {
                let frame: [f32; FRAME_SIZE_48K] = std::array::from_fn(|i| {
                    let t = (k * FRAME_SIZE_48K + i) as f32 / 48_000.0;
                    0.3 * (t * 300.0 * 2.0 * std::f32::consts::PI).sin()
                        + 0.2 * (t * 1200.0 * 2.0 * std::f32::consts::PI).sin()
                });
                encoder.encode(&frame).unwrap()
            })
            .collect();
        let mut out = [0.0f32; FRAME_SIZE_48K];
        for packet in &packets[..5] {
            decoder.decode(packet, &mut out).unwrap();
        }
        // Slot 5 is lost; packet 6 carries its in-band FEC.
        let decoded = decoder.decode_fec(&packets[6], &mut out).unwrap();
        assert_eq!(decoded, FRAME_SIZE_48K);
        assert!(out.iter().any(|sample| sample.abs() > 1e-3), "FEC output must not be silent");
    }
}
