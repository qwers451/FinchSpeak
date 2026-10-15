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

    pub fn set_packet_loss_perc(&mut self, loss_perc: u8) -> Result<(), AudioOpusError> {
        self.encoder.set_packet_loss_perc(loss_perc)?;
        Ok(())
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
}
