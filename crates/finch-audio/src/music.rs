//! File music decoding happens on a worker, outside the real-time audio callback.

use std::{collections::VecDeque, fs::File, io::Cursor, path::Path};
use rubato::{FftFixedInOut, Resampler};
use symphonia::core::{
    audio::SampleBuffer, codecs::{CodecParameters, Decoder, DecoderOptions, CODEC_TYPE_MP3, CODEC_TYPE_NULL},
    errors::Error, formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream}, meta::{MetadataOptions, MetadataRevision, StandardTagKey},
    probe::Hint, units::Time,
};

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub duration_ms: u64,
    pub title: Option<String>,
    pub artist: Option<String>,
}

fn reader(source: Box<dyn MediaSource>) -> Result<symphonia::core::probe::ProbeResult, String> {
    symphonia::default::get_probe().format(&Hint::new(), MediaSourceStream::new(source, Default::default()),
        // MPA may estimate n_frames from bitrate. Gapless trimming would use that
        // estimate as an EOF and discard valid VBR packets without Xing/VBRI.
        &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("Unsupported music format: {e}"))
}

fn read_tags(revision: &MetadataRevision, info: &mut TrackInfo) {
    for tag in revision.tags() {
        match tag.std_key {
            Some(StandardTagKey::TrackTitle) => info.title = Some(tag.value.to_string().trim_matches('\0').trim().to_string()),
            Some(StandardTagKey::Artist) => info.artist = Some(tag.value.to_string().trim_matches('\0').trim().to_string()),
            _ => {}
        }
    }
}

fn validate(params: &CodecParameters) -> Result<u32, String> {
    let rate = params.sample_rate.ok_or("Music sample rate is missing")?;
    if !(8_000..=192_000).contains(&rate) { return Err("Unsupported music sample rate".into()); }
    let channels = params.channels.ok_or("Music channels are missing")?.count();
    if !(1..=2).contains(&channels) { return Err("Music must be mono or stereo".into()); }
    Ok(rate)
}

pub fn probe(bytes: &[u8]) -> Result<TrackInfo, String> {
    let mut probed = reader(Box::new(Cursor::new(bytes.to_vec())))?;
    let mut info = TrackInfo { duration_ms: 0, title: None, artist: None };
    if let Some(metadata) = probed.metadata.get() && let Some(revision) = metadata.current() { read_tags(revision, &mut info); }
    if let Some(revision) = probed.format.metadata().current() { read_tags(revision, &mut info); }
    let track = probed.format.default_track().filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("Unsupported music format")?;
    // Construct a decoder as well: an Ogg container may contain an unsupported codec.
    symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| e.to_string())?;
    let track_id = track.id;
    let rate = validate(&track.codec_params)?;
    let time_base = track.codec_params.time_base;
    let mut end = if track.codec_params.codec == CODEC_TYPE_MP3 { 0 } else { track.codec_params.n_frames.unwrap_or(0) };
    if end == 0 {
        loop {
            match probed.format.next_packet() {
                Ok(packet) if packet.track_id() == track_id => end = end.max(packet.ts().saturating_add(packet.dur())),
                Ok(_) => {},
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    info.duration_ms = if let Some(base) = time_base {
        let time = base.calc_time(end);
        time.seconds.saturating_mul(1000).saturating_add((time.frac * 1000.0).round() as u64)
    } else { end.saturating_mul(1000) / u64::from(rate) };
    if info.duration_ms == 0 { return Err("Music track is empty".into()); }
    Ok(info)
}

pub struct MusicDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    input: [VecDeque<f32>; 2],
    output: VecDeque<[f32; 2]>,
    resampler: Option<FftFixedInOut<f32>>,
    skip_input: usize,
    skip_output: usize,
    remaining: Option<u64>,
    decoded_source: u64,
    emitted: u64,
    source_rate: u32,
    eof: bool,
    flushed: bool,
}
impl MusicDecoder {
    pub fn open(path: &Path, start_ms: u64) -> Result<Self, String> {
        let mut format = reader(Box::new(File::open(path).map_err(|e| e.to_string())?))?.format;
        let track = format.default_track().ok_or("Music track is missing")?;
        let track_id = track.id;
        let params = track.codec_params.clone();
        let rate = validate(&params)?;
        let decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(|e| e.to_string())?;
        let mut skip_input = 0;
        if start_ms != 0 {
            let seeked = format.seek(SeekMode::Accurate, SeekTo::Time { time: Time::from(start_ms as f64 / 1000.0), track_id: Some(track_id) }).map_err(|e| e.to_string())?;
            let ticks = seeked.required_ts.saturating_sub(seeked.actual_ts);
            skip_input = if let Some(base) = params.time_base {
                let t = base.calc_time(ticks);
                ((t.seconds as f64 + t.frac) * f64::from(rate)).round() as usize
            } else { ticks as usize };
        }
        let resampler = if rate == 48_000 { None } else {
            Some(FftFixedInOut::new(rate as usize, 48_000, rate as usize / 50, 2).map_err(|e| e.to_string())?)
        };
        let skip_output = resampler.as_ref().map_or(0, |r| r.output_delay());
        let remaining = if params.codec == CODEC_TYPE_MP3 { None } else { params.n_frames.map(|n| (n.saturating_mul(48_000) / u64::from(rate)).saturating_sub(start_ms.saturating_mul(48))) };
        Ok(Self { format, decoder, track_id, input: Default::default(), output: VecDeque::new(), resampler, skip_input, skip_output, remaining, decoded_source: 0, emitted: 0, source_rate: rate, eof: false, flushed: false })
    }

    fn decode_packet(&mut self) -> Result<(), String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.eof = true;
                    if self.remaining.is_none() { self.remaining = Some((self.decoded_source * 48_000 / u64::from(self.source_rate)).saturating_sub(self.emitted)); }
                    return Ok(());
                },
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id() != self.track_id { continue; }
            let audio = match self.decoder.decode(&packet) {
                Ok(audio) => audio,
                Err(Error::DecodeError(_)) => continue,
                Err(e) => return Err(e.to_string()),
            };
            let channels = audio.spec().channels.count();
            if channels == 0 || channels > 2 { return Err("Music must be mono or stereo".into()); }
            let mut samples = SampleBuffer::<f32>::new(audio.capacity() as u64, *audio.spec());
            samples.copy_interleaved_ref(audio);
            for frame in samples.samples().chunks_exact(channels) {
                if self.skip_input > 0 { self.skip_input -= 1; continue; }
                self.input[0].push_back(frame[0]);
                self.input[1].push_back(frame[channels - 1]);
                self.decoded_source += 1;
            }
            return Ok(());
        }
    }

    fn fill_output(&mut self) -> Result<(), String> {
        if self.flushed { return Ok(()); }
        let needed = self.resampler.as_ref().map_or(960, |r| r.input_frames_next());
        while self.input[0].len() < needed && !self.eof { self.decode_packet()?; }
        let available = self.input[0].len().min(needed);
        if available == 0 && self.eof && self.resampler.is_none() { self.flushed = true; return Ok(()); }
        let mut planes = [vec![0.0; needed], vec![0.0; needed]];
        for (plane, input) in planes.iter_mut().zip(&mut self.input) {
            for sample in plane.iter_mut().take(available) { if let Some(value) = input.pop_front() { *sample = value; } }
        }
        let converted = if let Some(resampler) = &mut self.resampler {
            resampler.process(&planes, None).map_err(|e| e.to_string())?
        } else { vec![planes[0][..available].to_vec(), planes[1][..available].to_vec()] };
        for (&left, &right) in converted[0].iter().zip(&converted[1]) {
            if self.skip_output > 0 { self.skip_output -= 1; } else { self.output.push_back([left, right]); }
        }
        if self.eof && available < needed {
            // One padded final block flushes the FFT filter tail. Known duration clips padding.
            if available == 0 { self.flushed = true; }
        }
        Ok(())
    }

    pub fn read_frames(&mut self, out: &mut [[f32; 2]]) -> Result<usize, String> {
        let mut count = 0;
        for frame in out {
            if self.remaining == Some(0) { break; }
            while self.output.is_empty() && !self.flushed { self.fill_output()?; }
            let Some(next) = self.output.pop_front() else { break; };
            *frame = next;
            self.emitted += 1;
            if let Some(remaining) = &mut self.remaining { *remaining = remaining.saturating_sub(1); }
            count += 1;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, milliseconds: u32) -> Vec<u8> {
        let frames = rate * milliseconds / 1000;
        let bytes = frames * u32::from(channels) * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF"); out.extend_from_slice(&(bytes + 36).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0");
        out.extend_from_slice(&channels.to_le_bytes()); out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes()); out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data"); out.extend_from_slice(&bytes.to_le_bytes());
        for frame in 0..frames {
            for channel in 0..channels {
                let sample: i16 = if channel == 0 { if frame < rate / 2 { 8192 } else { 16384 } } else { -8192 };
                out.extend_from_slice(&sample.to_le_bytes());
            }
        }
        out
    }

    fn with_file(bytes: &[u8], f: impl FnOnce(&std::path::Path)) {
        let path = std::env::temp_dir().join(format!("finch-music-{}-{:?}.wav", std::process::id(), std::thread::current().id()));
        std::fs::write(&path, bytes).unwrap(); f(&path); let _ = std::fs::remove_file(path);
    }

    #[test]
    fn music_probe_reports_duration_and_rejects_unknown_bytes() {
        assert_eq!(probe(&wav(48_000, 2, 1000)).unwrap().duration_ms, 1000);
        assert!(probe(b"not audio").is_err());
    }

    #[test]
    fn music_real_formats_keep_metadata_and_decode_after_seek() {
        for (name, bytes) in [
            ("wav", include_bytes!("../tests/fixtures/music/test.wav").as_slice()),
            ("mp3", include_bytes!("../tests/fixtures/music/test.mp3").as_slice()),
            ("ogg", include_bytes!("../tests/fixtures/music/test.ogg").as_slice()),
            ("flac", include_bytes!("../tests/fixtures/music/test.flac").as_slice()),
        ] {
            let info = probe(bytes).unwrap();
            assert!(info.duration_ms.abs_diff(2000) < 60, "{name}: {}", info.duration_ms);
            assert_eq!(info.title.as_deref(), Some("Finch test"), "{name}");
            assert_eq!(info.artist.as_deref(), Some("FinchSpeak QA"), "{name}");
            with_file(bytes, |path| {
                let mut decoder = MusicDecoder::open(path, 1000).unwrap();
                let mut frames = [[0.0; 2]; 960];
                assert_eq!(decoder.read_frames(&mut frames).unwrap(), 960, "{name}");
                assert!(frames.iter().any(|frame| frame[0].abs() > 0.05), "{name}");
                assert!(frames.iter().any(|frame| (frame[0] - frame[1]).abs() > 0.01), "{name} retains stereo");
            });
        }
    }

    #[test]
    fn music_vbr_mp3_without_xing_uses_real_duration_and_reaches_the_end() {
        let bytes = include_bytes!("../tests/fixtures/music/vbr-no-xing.mp3");
        let info = probe(bytes).unwrap();
        assert!(info.duration_ms.abs_diff(6000) < 70, "duration {}", info.duration_ms);
        with_file(bytes, |path| {
            let mut decoder = MusicDecoder::open(path, 0).unwrap();
            let mut frames = [[0.0; 2]; 960]; let mut total = 0;
            loop { let n = decoder.read_frames(&mut frames).unwrap(); if n == 0 { break; } total += n; }
            assert!(total.abs_diff(6 * 48_000) < 3500, "decoded {total} samples");
        });
    }

    #[test]
    fn music_probe_rejects_formats_the_decoder_cannot_play() {
        assert!(probe(&wav(48_000, 6, 1000)).is_err(), "5.1 audio cannot be played by the stereo decoder");
        assert!(probe(&wav(384_000, 2, 1000)).is_err(), "unsupported resampling rate must fail before upload");
    }

    #[test]
    fn music_preserves_stereo_and_seeks_to_current_position() {
        with_file(&wav(48_000, 2, 1000), |path| {
            let mut decoder = MusicDecoder::open(path, 600).unwrap();
            let mut frames = [[0.0; 2]; 960];
            assert_eq!(decoder.read_frames(&mut frames).unwrap(), 960);
            assert!((frames[500][0] - 0.5).abs() < 0.01);
            assert!((frames[500][1] + 0.25).abs() < 0.01);
        });
    }

    #[test]
    fn music_resamples_mono_to_48k_stereo_with_bounded_chunks() {
        with_file(&wav(44_100, 1, 1000), |path| {
            let mut decoder = MusicDecoder::open(path, 0).unwrap();
            let mut frames = [[0.0; 2]; 960];
            let mut count = 0;
            loop {
                let n = decoder.read_frames(&mut frames).unwrap();
                if n == 0 { break; }
                count += n;
                assert!(frames[..n].iter().all(|frame| (frame[0] - frame[1]).abs() < 0.0001));
            }
            assert_eq!(count, 48_000);
        });
    }
}
