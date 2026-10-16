//! Multi-speaker audio mixer with `tanh()` soft-limiting and local speech tracking.
//!
//! - Mixes up to N participants every 20 ms.
//! - Per-speaker volume scaling (0.0 to 2.0).
//! - Non-linear `x.tanh()` soft limiter prevents harsh clipping when multiple people talk/shout.
//! - Tracks speaker activity locally (250 ms timeout or silence flag triggers UI speaking border events).

use std::collections::hash_map::{Entry, HashMap};
use std::time::{Duration, Instant};

use bytes::Bytes;
use finch_protocol::media::MediaHeader;

use crate::jitter::{JitterBuffer, PlayoutResult};
use crate::resampler::FRAME_SIZE_48K;

/// Timeout after which a speaker is considered silent if no packets arrive (250 ms).
pub const SPEECH_TIMEOUT: Duration = Duration::from_millis(250);

/// Mixer key of the screen-share sound of `peer_id`: separate from the person's voice so it gets
/// its own volume and never counts as speech.
pub const fn screen_audio_key(peer_id: u32) -> u32 {
    peer_id | SCREEN_AUDIO_FLAG
}

const SCREEN_AUDIO_FLAG: u32 = 0x4000_0000;

/// State for a single remote speaker.
pub struct RemoteSpeaker {
    pub peer_id: u32,
    pub volume: f32,
    pub jitter_buffer: JitterBuffer,
    pub last_packet_time: Instant,
    pub is_speaking: bool,
}

/// Multi-speaker audio mixer.
pub struct AudioMixer {
    speakers: HashMap<u32, RemoteSpeaker>,
    /// Volumes chosen before the speaker's first packet (e.g. remembered from the last call).
    preset_volumes: HashMap<u32, f32>,
    scratch_buf: [f32; FRAME_SIZE_48K],
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioMixer {
    pub fn new() -> Self {
        Self {
            speakers: HashMap::new(),
            preset_volumes: HashMap::new(),
            scratch_buf: [0.0f32; FRAME_SIZE_48K],
        }
    }

    /// Add or update a speaker's volume multiplier (0.0 to 2.0).
    pub fn set_speaker_volume(&mut self, peer_id: u32, volume: f32) {
        let clamped = volume.clamp(0.0, 2.0);
        // Remembered even if the speaker has not sent anything yet.
        self.preset_volumes.insert(peer_id, clamped);
        if let Some(speaker) = self.speakers.get_mut(&peer_id) {
            speaker.volume = clamped;
        }
    }

    /// Remove a speaker who left the voice channel.
    pub fn remove_speaker(&mut self, peer_id: u32) {
        self.speakers.remove(&peer_id);
        self.preset_volumes.remove(&peer_id);
    }

    /// Largest playout queue length across all remote speakers.
    pub fn max_buffered_packets(&self) -> usize {
        self.speakers
            .values()
            .map(|s| s.jitter_buffer.buffered_len())
            .max()
            .unwrap_or(0)
    }

    /// Packet loss rate observed for a speaker, if that speaker is known.
    pub fn loss_rate(&self, peer_id: u32) -> Option<f64> {
        self.speakers.get(&peer_id).map(|s| s.jitter_buffer.loss_rate())
    }

    /// `(peer id, packets expected, packets lost)` of every remote voice (screen sound excluded).
    pub fn loss_counters(&self) -> Vec<(u32, u64, u64)> {
        self.speakers
            .iter()
            .filter(|(key, _)| **key & SCREEN_AUDIO_FLAG == 0)
            .map(|(key, speaker)| {
                let (expected, lost) = speaker.jitter_buffer.counters();
                (*key, expected, lost)
            })
            .collect()
    }

    /// Feed an incoming audio packet from the network into the appropriate speaker's jitter buffer.
    /// Returns `Some((peer_id, speaking))` if a UI speech state transition occurred.
    pub fn on_audio_packet(
        &mut self,
        header: &MediaHeader,
        payload: Bytes,
        now: Instant,
    ) -> Option<(u32, bool)> {
        let peer_id = header.sender_peer_id;
        let is_silence = header.is_silence();

        let speaker = match self.speakers.entry(peer_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let jitter_buffer = match JitterBuffer::new() {
                    Ok(jb) => jb,
                    Err(err) => {
                        tracing::error!(
                            peer_id,
                            %err,
                            "Failed to initialize JitterBuffer decoder for speaker"
                        );
                        return None;
                    }
                };
                entry.insert(RemoteSpeaker {
                    peer_id,
                    volume: self.preset_volumes.get(&peer_id).copied().unwrap_or(1.0),
                    jitter_buffer,
                    last_packet_time: now,
                    is_speaking: false,
                })
            }
        };

        speaker.last_packet_time = now;
        speaker.jitter_buffer.push(header, payload, now);

        if is_silence {
            if speaker.is_speaking {
                speaker.is_speaking = false;
                return Some((peer_id, false));
            }
        } else if !speaker.is_speaking {
            speaker.is_speaking = true;
            return Some((peer_id, true));
        }

        None
    }

    /// Feed a `ScreenAudio` packet (stereo Opus; decoded to mono) into the screen channel of its
    /// sender. Never reports speech.
    pub fn on_screen_audio_packet(&mut self, header: &MediaHeader, payload: Bytes, now: Instant) -> Option<(u32, bool)> {
        let mut as_voice = *header;
        as_voice.media_type = finch_protocol::media::MediaType::Audio;
        as_voice.sender_peer_id = screen_audio_key(header.sender_peer_id);
        self.on_audio_packet(&as_voice, payload, now);
        None
    }

    /// Mix all remote speakers into a 960-sample 48 kHz output buffer.
    /// Applies individual volume adjustments and `tanh()` soft-limiting.
    /// Returns a list of speakers whose speech status changed (e.g. timed out after 250 ms).
    pub fn mix_frame(
        &mut self,
        output: &mut [f32; FRAME_SIZE_48K],
        now: Instant,
    ) -> Vec<(u32, bool)> {
        output.fill(0.0);
        let mut speech_events = Vec::new();

        for speaker in self.speakers.values_mut() {
            // Check for 250 ms silence timeout
            if speaker.is_speaking && now.saturating_duration_since(speaker.last_packet_time) > SPEECH_TIMEOUT {
                speaker.is_speaking = false;
                if speaker.peer_id & SCREEN_AUDIO_FLAG == 0 {
                    speech_events.push((speaker.peer_id, false));
                }
            }

            // Pop next decoded 20 ms frame
            let result = speaker.jitter_buffer.pop_frame(&mut self.scratch_buf);
            if result == PlayoutResult::Normal || result == PlayoutResult::Concealed {
                let vol = speaker.volume;
                for (out_sample, &in_sample) in output.iter_mut().zip(self.scratch_buf.iter()) {
                    *out_sample += in_sample * vol;
                }
            }
        }

        // Apply tanh() soft-limiting: compresses loud peaks without hard digital clipping
        for sample in output.iter_mut() {
            *sample = sample.tanh();
        }

        speech_events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opus::OpusVoiceEncoder;

    #[test]
    fn test_mixer_soft_limiting() {
        let mut mixer = AudioMixer::new();
        let now = Instant::now();

        let mut encoder = OpusVoiceEncoder::new(64).unwrap();
        // Loud test signal
        let loud = [0.9f32; FRAME_SIZE_48K];
        let opus_data = encoder.encode(&loud).unwrap();

        // 3 loud speakers simultaneously
        for peer_id in 1..=3 {
            let header = MediaHeader::new_audio(1, peer_id, 1, 0, false, false);
            mixer.on_audio_packet(&header, opus_data.clone(), now);
        }

        let mut out = [0.0f32; FRAME_SIZE_48K];
        let _ = mixer.mix_frame(&mut out, now);

        // Verify tanh() keeps all samples strictly within (-1.0, 1.0)
        for &s in &out {
            assert!(s.abs() < 1.0);
        }
    }

    #[test]
    fn test_on_audio_packet_speech_transitions() {
        let mut mixer = AudioMixer::new();
        let now = Instant::now();
        let payload = Bytes::from_static(&[0u8; 10]);

        // First packet (normal speech) triggers transition to speaking: true
        let normal_hdr = MediaHeader::new_audio(1, 42, 1, 0, false, false);
        let event = mixer.on_audio_packet(&normal_hdr, payload.clone(), now);
        assert_eq!(event, Some((42, true)));

        // Subsequent normal packet from same speaker does not generate transition event
        let next_hdr = MediaHeader::new_audio(1, 42, 2, 20, false, false);
        let event = mixer.on_audio_packet(&next_hdr, payload.clone(), now);
        assert_eq!(event, None);

        // Silence packet triggers transition to speaking: false
        let silence_hdr = MediaHeader::new_audio(1, 42, 3, 1920, true, false);
        let event = mixer.on_audio_packet(&silence_hdr, payload, now);
        assert_eq!(event, Some((42, false)));
    }

    #[test]
    fn test_speech_timeout_in_mix_frame() {
        let mut mixer = AudioMixer::new();
        let now = Instant::now();
        let payload = Bytes::from_static(&[0u8; 10]);

        // Start speaking
        let normal_hdr = MediaHeader::new_audio(1, 100, 1, 0, false, false);
        let _ = mixer.on_audio_packet(&normal_hdr, payload, now);

        let mut out = [0.0f32; FRAME_SIZE_48K];

        // Before 250 ms timeout: no event
        let events = mixer.mix_frame(&mut out, now + Duration::from_millis(200));
        assert!(events.is_empty());

        // After 250 ms timeout: speaker 100 is marked silent
        let events = mixer.mix_frame(&mut out, now + Duration::from_millis(260));
        assert_eq!(events, vec![(100, false)]);
    }

    #[test]
    fn test_buffer_and_loss_accessors() {
        let mut mixer = AudioMixer::new();
        let now = Instant::now();
        let mut encoder = OpusVoiceEncoder::new(64).unwrap();
        let payload = encoder.encode(&[0.1f32; FRAME_SIZE_48K]).unwrap();

        assert_eq!(mixer.max_buffered_packets(), 0);
        assert_eq!(mixer.loss_rate(7), None);

        // Sequences 1, 2, 4 queued for peer 7; 3 is missing.
        for seq in [1u16, 2, 4] {
            let header = MediaHeader::new_audio(1, 7, seq, seq as u32 * 20, false, false);
            mixer.on_audio_packet(&header, payload.clone(), now);
        }
        assert_eq!(mixer.max_buffered_packets(), 3);

        let mut out = [0.0f32; FRAME_SIZE_48K];
        for _ in 0..4 {
            let _ = mixer.mix_frame(&mut out, now);
        }
        let loss = mixer.loss_rate(7).unwrap();
        assert!(loss > 0.0 && loss < 1.0, "loss rate {loss}");
    }

    #[test]
    fn loss_counters_list_voices_but_not_screen_sound() {
        let mut mixer = AudioMixer::new();
        let now = Instant::now();
        let mut encoder = crate::opus::OpusVoiceEncoder::new(64).unwrap();
        let payload = encoder.encode(&[0.1f32; FRAME_SIZE_48K]).unwrap();
        let voice = MediaHeader::new_audio(1, 7, 0, 0, false, false);
        mixer.on_audio_packet(&voice, payload.clone(), now);
        let screen = MediaHeader::new_audio(1, 8, 0, 0, false, false);
        mixer.on_screen_audio_packet(&screen, payload, now);
        let peers: Vec<u32> = mixer.loss_counters().iter().map(|(peer, _, _)| *peer).collect();
        assert_eq!(peers, vec![7]);
    }

    #[test]
    fn a_volume_chosen_before_the_first_packet_is_used_when_the_speaker_appears() {
        let mut mixer = AudioMixer::new();
        mixer.set_speaker_volume(7, 1.5);
        mixer.set_speaker_volume(8, 9.0); // out of range: clamped
        let mut encoder = crate::opus::OpusVoiceEncoder::new(64).unwrap();
        let payload = encoder.encode(&[0.1f32; FRAME_SIZE_48K]).unwrap();
        let now = Instant::now();
        mixer.on_audio_packet(&MediaHeader::new_audio(1, 7, 0, 0, false, false), payload.clone(), now);
        mixer.on_audio_packet(&MediaHeader::new_audio(1, 8, 0, 0, false, false), payload, now);
        assert_eq!(mixer.speakers[&7].volume, 1.5);
        assert_eq!(mixer.speakers[&8].volume, 2.0);
        mixer.remove_speaker(7);
        assert!(!mixer.preset_volumes.contains_key(&7), "a speaker who left is forgotten");
    }
}
