//! Capture provenance in the device clock domain. No allocation or locks in callbacks.
#[derive(Clone, Copy, Default)]
pub(crate) struct CapturedSample {
    pub sample: f32,
    pub generation: u64,
}

pub(crate) struct CaptureClock<T> {
    generation: u64,
    first_seen: bool,
    cutoff: Option<T>,
    previous: Option<(T, T)>,
}
impl<T: Copy + Ord> Default for CaptureClock<T> {
    fn default() -> Self {
        Self {
            generation: 0,
            first_seen: false,
            cutoff: None,
            previous: None,
        }
    }
}
impl<T: Copy + Ord> CaptureClock<T> {
    pub fn tag(&mut self, generation: u64, open: bool, capture: T, callback: T) -> u64 {
        if self.generation != generation || !open {
            self.generation = generation;
            self.first_seen = false;
            self.cutoff = None;
            self.previous = None;
        }
        if !open || generation == 0 || capture > callback {
            return 0;
        }
        if let Some((last_capture, last_callback)) = self.previous {
            if capture <= last_capture || callback <= last_callback {
                return 0;
            }
        }
        self.previous = Some((capture, callback));
        if !self.first_seen {
            // This callback's timestamp may have been taken before the control transition.
            self.first_seen = true;
            return 0;
        }
        let Some(cutoff) = self.cutoff else {
            // Callbacks of one stream are serial. This timestamp was taken after the
            // preceding handler observed this generation. Discard this block too.
            self.cutoff = Some(callback);
            return 0;
        };
        if capture >= cutoff {
            generation
        } else {
            0
        }
    }
}

/// A resampler input chunk must consist entirely of proven-fresh samples.
pub(crate) fn public_chunk(samples: &[CapturedSample], generation: u64) -> bool {
    !samples.is_empty() && generation != 0 && samples.iter().all(|s| s.generation == generation)
}

/// Reset partial FFT history whenever a chunk switches provenance. Private samples
/// remain available for the local test/meter, but cannot seed the next public chunk.
#[derive(Default)]
pub(crate) struct InputBoundary {
    generation: u64,
    public: bool,
}
impl InputBoundary {
    pub fn prepare(
        &mut self,
        samples: &[CapturedSample],
        generation: u64,
        open: bool,
        resampler: &mut finch_audio::InputResampler,
    ) -> bool {
        let public = open && public_chunk(samples, generation);
        if self.generation != generation || self.public != public {
            resampler.reset();
        }
        self.generation = generation;
        self.public = public;
        public
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_privacy_callback_cutoff_rejects_backlog_and_invalid_clocks() {
        let mut clock = CaptureClock::default();
        assert_eq!(clock.tag(16, false, 1, 10), 0);
        // First callback can have sampled its timestamp before the transition.
        assert_eq!(clock.tag(32, true, 2, 11), 0);
        assert_eq!(clock.tag(32, true, 3, 20), 0);
        assert_eq!(clock.tag(32, true, 19, 21), 0); // old hardware backlog
        assert_eq!(clock.tag(32, true, 20, 22), 32);
        assert_eq!(clock.tag(32, true, 20, 23), 0); // nonadvancing capture clock
        assert_eq!(clock.tag(32, true, 24, 23), 0); // invalid capture > callback
        assert_eq!(clock.tag(48, true, 30, 40), 0); // new epoch, even still open
        assert_eq!(clock.tag(48, true, 31, 41), 0);
        assert_eq!(clock.tag(48, true, 41, 42), 48);
    }
    #[test]
    fn capture_privacy_inflight_callback_keeps_its_original_generation() {
        use ringbuf::{traits::*, HeapRb};
        let (mut producer, mut consumer) = HeapRb::<CapturedSample>::new(2048).split();
        let (half_tx, half_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut clock = CaptureClock::default();
            let tag = clock.tag(16, false, 1, 10);
            for _ in 0..480 {
                producer
                    .try_push(CapturedSample {
                        sample: 0.75,
                        generation: tag,
                    })
                    .unwrap_or_else(|_| panic!());
            }
            half_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
            for _ in 0..480 {
                producer
                    .try_push(CapturedSample {
                        sample: 0.75,
                        generation: tag,
                    })
                    .unwrap_or_else(|_| panic!());
            }
            for _ in 0..480 {
                producer
                    .try_push(CapturedSample {
                        sample: 0.0,
                        generation: 32,
                    })
                    .unwrap_or_else(|_| panic!());
            }
        });
        half_rx.recv().unwrap();
        consumer.clear();
        resume_tx.send(()).unwrap();
        thread.join().unwrap();
        let samples: Vec<_> = consumer.pop_iter().collect();
        assert_eq!(samples.len(), 960);
        assert!(!public_chunk(&samples, 32));
        assert_eq!(samples.iter().filter(|s| s.sample == 0.75).count(), 480);
        assert!(samples[..480].iter().all(|s| s.generation == 0));
    }
    #[test]
    fn capture_privacy_fast_controls_have_distinct_generations() {
        let settings = crate::state::VoiceSettings::default();
        let start = settings.privacy_snapshot();
        settings.set_muted(true);
        settings.set_muted(false);
        assert!(settings.privacy_snapshot().1);
        assert_ne!(start.0, settings.privacy_snapshot().0);
        let mut dto = settings.snapshot();
        dto.push_to_talk = true;
        settings.apply(&dto);
        settings.set_key_down(true);
        let start = settings.privacy_snapshot();
        settings.set_key_down(false);
        settings.set_key_down(true);
        assert_eq!(start.1, settings.privacy_snapshot().1);
        assert_ne!(start.0, settings.privacy_snapshot().0);
        let start = settings.privacy_snapshot();
        settings.set_mic_test(true);
        settings.set_mic_test(false);
        assert_ne!(start.0, settings.privacy_snapshot().0);
        let start = settings.privacy_snapshot();
        dto.voice_enhancement = false;
        dto.input_gain = 2.0;
        settings.apply(&dto);
        assert_eq!(
            start,
            settings.privacy_snapshot(),
            "gain/toggle never reset AEC/privacy history"
        );
    }

    #[test]
    fn capture_privacy_marker_cannot_reach_opus_after_mixed_or_delayed_input() {
        use finch_audio::{
            InputResampler, VadMode, VoiceCapture, VoiceCaptureConfig, FRAME_SIZE_48K,
        };
        // Both the no-resampler and partial FFT-history paths, with/without existing codec state.
        for enabled in [false, true] {
            for rate in [48_000, 44_100] {
                for prior_public in [false, true] {
                    let config = VoiceCaptureConfig {
                        voice_enhancement: enabled,
                        vad_mode: VadMode::Energy,
                        vad_threshold_db: -140.0,
                        ..Default::default()
                    };
                    let mut captures = [
                        VoiceCapture::new(config).unwrap(),
                        VoiceCapture::new(config).unwrap(),
                    ];
                    let mut resamplers = [
                        InputResampler::new(rate).unwrap(),
                        InputResampler::new(rate).unwrap(),
                    ];
                    let mut boundaries = [InputBoundary::default(), InputBoundary::default()];
                    let mut clocks = [CaptureClock::default(), CaptureClock::default()];
                    let mut public_packets = 0;
                    for index in 0..145 {
                        let (generation, open, public) = if index < 100 {
                            (16, prior_public, prior_public)
                        } else if index < 120 {
                            (32, false, false)
                        } else {
                            (48, true, index >= 124)
                        };
                        let mut packets = Vec::new();
                        for side in 0..2 {
                            let needed = resamplers[side].input_frames_needed();
                            let time = index as u64 * 100;
                            // Callback 122 arrives after reopening but still carries pre-cutoff audio.
                            let capture_time = if index == 122 { 12050 } else { time };
                            let tag = clocks[side].tag(generation, open, capture_time, time + 50);
                            let samples: Vec<_> = (0..needed)
                                .map(|sample| {
                                    let private = index >= 100 && !public;
                                    let marker = (2.0
                                        * std::f32::consts::PI
                                        * 313.0
                                        * (index * needed + sample) as f32
                                        / rate as f32)
                                        .sin()
                                        * 0.6;
                                    CapturedSample {
                                        sample: if index < 100 && prior_public
                                            || private && side == 0
                                        {
                                            marker
                                        } else {
                                            0.0
                                        },
                                        // Mixed partial chunk cannot inherit the new public tag.
                                        generation: if index == 123 && sample < needed / 2 {
                                            0
                                        } else {
                                            tag
                                        },
                                    }
                                })
                                .collect();
                            let fresh = boundaries[side].prepare(
                                &samples,
                                generation,
                                open,
                                &mut resamplers[side],
                            );
                            captures[side].set_transmit_gate(fresh);
                            let raw: Vec<_> = samples.iter().map(|s| s.sample).collect();
                            let mut frame = [0.0; FRAME_SIZE_48K];
                            resamplers[side].process(&raw, &mut frame).unwrap();
                            let output = captures[side]
                                .process(&mut frame, 1, index as u32 * 20)
                                .unwrap();
                            if !public {
                                assert!(
                                    output.packets.is_empty(),
                                    "private/backlog/mixed frame {index}"
                                );
                            }
                            if side == 0 {
                                public_packets += output.packets.len();
                            }
                            packets.push(output.packets);
                        }
                        if public {
                            assert_eq!(packets[0], packets[1], "marker altered public Opus: enabled={enabled}, rate={rate}, prior={prior_public}, frame={index}");
                        }
                    }
                    assert!(
                        public_packets > 0,
                        "test must actually reopen and encode silence"
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "requires an actual microphone; run explicitly on the local QA machine"]
    fn capture_privacy_real_device_clock_accepts_fresh_audio() {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let settings = Arc::new(crate::state::VoiceSettings::default());
        let accepted = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(AtomicUsize::new(0));
        let device = cpal::default_host()
            .default_input_device()
            .expect("microphone required");
        let config = device.default_input_config().unwrap();
        let callback_settings = settings.clone();
        let count = accepted.clone();
        let failed = errors.clone();
        let mut clock = CaptureClock::default();
        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], info: &cpal::InputCallbackInfo| {
                    let (generation, open) = callback_settings.privacy_snapshot();
                    let ts = info.timestamp();
                    if !data.is_empty() && clock.tag(generation, open, ts.capture, ts.callback) != 0
                    {
                        count.fetch_add(1, Ordering::Relaxed);
                    }
                },
                move |_| {
                    failed.fetch_add(1, Ordering::Relaxed);
                },
                None,
            )
            .unwrap();
        stream.play().unwrap();
        for _ in 0..3 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            assert!(
                accepted.swap(0, Ordering::SeqCst) > 0,
                "valid hardware timestamps must reopen capture"
            );
            settings.set_muted(true);
            std::thread::sleep(std::time::Duration::from_millis(100));
            accepted.store(0, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert_eq!(
                accepted.load(Ordering::SeqCst),
                0,
                "closed microphone must not be tagged public"
            );
            settings.set_muted(false);
            std::thread::sleep(std::time::Duration::from_millis(500));
            assert!(
                accepted.swap(0, Ordering::SeqCst) > 0,
                "each reopening must accept fresh hardware audio"
            );
        }
        drop(stream);
        assert_eq!(errors.load(Ordering::SeqCst), 0);
    }
}
