//! Screen share media path without a network: frames -> H.264 -> key frame / datagrams ->
//! viewer -> H.264 decoder, compared with the source pixels (AUD-018).

use std::time::{Duration, Instant};

use bytes::Bytes;
use finch_client_core::screen::{OutgoingVideo, ScreenPacketizer, ScreenViewer, ShareConfig};
use finch_protocol::media::{MediaHeader, MediaType, MAX_DATAGRAM_SIZE, MEDIA_HEADER_SIZE};
use finch_screen::{AssembledFrame, PixelFormat, RawFrame};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;

const CHANNEL: u32 = 2;
const SENDER: u32 = 1001;

/// A moving diagonal gradient with a bouncing block: easy to compress, never static.
fn test_frame(width: u32, height: u32, t: u32) -> RawFrame {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let base = ((x + y + t * 3) % 256) as u8;
            let in_block = x / 40 == (t * 2 / 40) % (width / 40) && y / 40 == 2;
            let (b, g, r) = if in_block { (255, 255, 255) } else { (base, 255 - base, 128) };
            data.extend_from_slice(&[b, g, r, 255]);
        }
    }
    RawFrame::new(width, height, PixelFormat::Bgra, Bytes::from(data))
}

/// Pseudo-random noise: incompressible, so a delta frame is huge.
fn noise_frame(width: u32, height: u32, seed: u32) -> RawFrame {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(12345);
    let data: Vec<u8> = (0..width * height * 4)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    RawFrame::new(width, height, PixelFormat::Bgra, Bytes::from(data))
}

/// `base` with its lower half replaced by fresh noise: a big delta, but no scene cut (a full
/// replacement makes the encoder emit a key frame instead).
fn half_changed(base: &RawFrame, seed: u32) -> RawFrame {
    let fresh = noise_frame(base.width, base.height, seed);
    let half = base.data.len() / 2;
    let mut data = base.data.to_vec();
    data[half..].copy_from_slice(&fresh.data[half..]);
    RawFrame::new(base.width, base.height, PixelFormat::Bgra, Bytes::from(data))
}

fn packetizer() -> ScreenPacketizer {
    ScreenPacketizer::new(ShareConfig { fps: 30, bitrate_bps: Some(4_000_000) }, CHANNEL, SENDER)
}

fn is_key(out: &OutgoingVideo) -> bool {
    matches!(out, OutgoingVideo::KeyFrame { .. })
}

fn header_of(datagram: &Bytes) -> MediaHeader {
    MediaHeader::from_slice(&datagram[..MEDIA_HEADER_SIZE]).expect("valid header")
}

#[test]
fn first_frame_is_a_keyframe_then_deltas() {
    let mut p = packetizer();
    let first = p.push_frame(&test_frame(320, 180, 0), 0, MAX_DATAGRAM_SIZE).unwrap();
    let OutgoingVideo::KeyFrame { header, data } = first else { panic!("first frame must be a key frame") };
    assert_eq!(header.media_type, MediaType::VideoKey);
    assert_eq!((header.channel_id, header.sender_peer_id), (CHANNEL, SENDER));
    assert!(!data.is_empty());

    let second = p.push_frame(&test_frame(320, 180, 1), 33, MAX_DATAGRAM_SIZE).unwrap();
    let OutgoingVideo::Delta(packets) = second else { panic!("second frame must be a delta") };
    assert!(!packets.is_empty());
    assert!(packets.iter().all(|d| header_of(d).media_type == MediaType::VideoDelta));
    assert_eq!(p.dimensions(), Some((320, 180)));
}

#[test]
fn a_requested_keyframe_comes_next() {
    let mut p = packetizer();
    p.push_frame(&test_frame(320, 180, 0), 0, MAX_DATAGRAM_SIZE).unwrap();
    assert!(!is_key(&p.push_frame(&test_frame(320, 180, 1), 33, MAX_DATAGRAM_SIZE).unwrap()));
    p.request_keyframe();
    assert!(is_key(&p.push_frame(&test_frame(320, 180, 2), 66, MAX_DATAGRAM_SIZE).unwrap()));
    assert!(!is_key(&p.push_frame(&test_frame(320, 180, 3), 99, MAX_DATAGRAM_SIZE).unwrap()));
}

#[test]
fn datagrams_respect_the_connection_limit() {
    for limit in [MAX_DATAGRAM_SIZE, 700, 400] {
        let mut p = packetizer();
        let base = noise_frame(640, 360, 1);
        p.push_frame(&base, 0, limit).unwrap();
        let OutgoingVideo::Delta(packets) = p.push_frame(&half_changed(&base, 2), 33, limit).unwrap() else {
            panic!("delta expected")
        };
        assert!(packets.len() > 1, "a noise frame needs several fragments at limit {limit}");
        assert!(packets.iter().all(|d| d.len() <= limit), "a datagram exceeds the limit {limit}");
    }
}

#[test]
fn a_frame_needing_more_than_255_fragments_is_dropped_and_a_keyframe_follows() {
    let mut p = ScreenPacketizer::new(ShareConfig { fps: 30, bitrate_bps: Some(30_000_000) }, CHANNEL, SENDER);
    let base = noise_frame(640, 360, 1);
    p.push_frame(&base, 0, 300).unwrap();
    // 300-byte datagrams carry 280 bytes each; this delta is far above 255 * 280 bytes.
    let huge = p.push_frame(&half_changed(&base, 7), 33, 300).unwrap();
    assert!(matches!(huge, OutgoingVideo::Dropped), "got {huge:?}");
    assert!(is_key(&p.push_frame(&test_frame(640, 360, 2), 66, 300).unwrap()), "the viewer must be resynchronised");
}

#[test]
fn odd_sized_frames_are_cropped_to_an_even_size() {
    let mut p = packetizer();
    let out = p.push_frame(&test_frame(321, 181, 0), 0, MAX_DATAGRAM_SIZE).unwrap();
    assert!(is_key(&out));
    assert_eq!(p.dimensions(), Some((320, 180)));
}

#[test]
fn a_resolution_change_restarts_with_a_keyframe_of_the_new_size() {
    let mut p = packetizer();
    p.push_frame(&test_frame(320, 180, 0), 0, MAX_DATAGRAM_SIZE).unwrap();
    assert!(!is_key(&p.push_frame(&test_frame(320, 180, 1), 33, MAX_DATAGRAM_SIZE).unwrap()));
    let out = p.push_frame(&test_frame(480, 270, 2), 66, MAX_DATAGRAM_SIZE).unwrap();
    assert!(is_key(&out), "a new size needs a new key frame");
    assert_eq!(p.dimensions(), Some((480, 270)));
}

// ---- viewer ---------------------------------------------------------------------------------

fn delta_header(frame_id: u16) -> MediaHeader {
    MediaHeader::new_video_delta(CHANNEL, SENDER, frame_id, 0, frame_id, 0, 1, false)
}

fn key_header(frame_id: u16) -> MediaHeader {
    MediaHeader::new_video_key(CHANNEL, SENDER, frame_id, 0, frame_id, false)
}

fn payload() -> Bytes {
    Bytes::from_static(&[1, 2, 3])
}

#[test]
fn viewer_drops_deltas_until_the_first_keyframe_and_asks_for_one() {
    let mut v = ScreenViewer::new();
    let t0 = Instant::now();
    assert!(v.take_keyframe_request(t0), "a new viewer needs a key frame");
    assert!(v.on_delta_fragment(&delta_header(5), payload(), t0).is_none());

    let key = v.on_keyframe(&key_header(10), payload(), t0);
    assert!(key.is_keyframe);
    assert!(!v.take_keyframe_request(t0 + Duration::from_secs(5)), "nothing is missing now");
    assert!(v.on_delta_fragment(&delta_header(11), payload(), t0).is_some());
}

#[test]
fn viewer_asks_at_most_once_per_second() {
    let mut v = ScreenViewer::new();
    let t0 = Instant::now();
    assert!(v.take_keyframe_request(t0));
    assert!(!v.take_keyframe_request(t0 + Duration::from_millis(900)));
    assert!(v.take_keyframe_request(t0 + Duration::from_millis(1100)), "still waiting after a second: ask again");
}

#[test]
fn viewer_notices_a_missing_frame_and_waits_for_a_new_keyframe() {
    let mut v = ScreenViewer::new();
    let t0 = Instant::now();
    v.on_keyframe(&key_header(10), payload(), t0);
    assert!(v.on_delta_fragment(&delta_header(11), payload(), t0).is_some());

    // Frame 12 never arrives: 13 cannot be decoded, nor anything after it.
    assert!(v.on_delta_fragment(&delta_header(13), payload(), t0).is_none());
    assert!(v.on_delta_fragment(&delta_header(14), payload(), t0).is_none());
    assert!(v.take_keyframe_request(t0));

    v.on_keyframe(&key_header(20), payload(), t0);
    assert!(v.on_delta_fragment(&delta_header(21), payload(), t0).is_some());
}

#[test]
fn viewer_notices_a_frame_that_never_completed() {
    let mut v = ScreenViewer::new();
    let t0 = Instant::now();
    v.on_keyframe(&key_header(10), payload(), t0);
    let first_of_two = MediaHeader::new_video_delta(CHANNEL, SENDER, 11, 0, 11, 0, 2, false);
    assert!(v.on_delta_fragment(&first_of_two, payload(), t0).is_none());

    v.tick(t0 + Duration::from_millis(200));
    assert!(v.take_keyframe_request(t0 + Duration::from_millis(200)));
}

#[test]
fn viewer_holds_deltas_while_a_keyframe_is_in_flight() {
    let mut v = ScreenViewer::new();
    let t0 = Instant::now();
    v.on_keyframe(&key_header(10), payload(), t0);

    v.keyframe_stream_started(t0);
    // These deltas reference the key frame that has not arrived yet.
    assert!(v.on_delta_fragment(&delta_header(21), payload(), t0).is_none());
    v.on_keyframe(&key_header(20), payload(), t0);
    assert!(v.on_delta_fragment(&delta_header(21), payload(), t0).is_some());
}

#[test]
fn viewer_gives_up_on_a_keyframe_stream_that_never_finishes() {
    let mut v = ScreenViewer::new();
    let t0 = Instant::now();
    v.on_keyframe(&key_header(10), payload(), t0);
    v.keyframe_stream_started(t0);

    v.tick(t0 + Duration::from_secs(4));
    assert!(v.take_keyframe_request(t0 + Duration::from_secs(4)), "ask for a new key frame");
    v.on_keyframe(&key_header(30), payload(), t0 + Duration::from_secs(5));
    assert!(v.on_delta_fragment(&delta_header(31), payload(), t0 + Duration::from_secs(5)).is_some());
}

// ---- the whole chain ------------------------------------------------------------------------

/// Run one outgoing item through the viewer; returns the frame ready for the decoder.
fn deliver(viewer: &mut ScreenViewer, out: OutgoingVideo, now: Instant) -> Vec<AssembledFrame> {
    match out {
        OutgoingVideo::KeyFrame { header, data } => vec![viewer.on_keyframe(&header, data, now)],
        OutgoingVideo::Delta(packets) => packets
            .into_iter()
            .filter_map(|d| viewer.on_delta_fragment(&header_of(&d), d.slice(MEDIA_HEADER_SIZE..), now))
            .collect(),
        OutgoingVideo::Dropped => Vec::new(),
    }
}

fn mean_abs_error(decoded_rgb: &[u8], source: &RawFrame) -> f64 {
    let bgra = source.to_bgra();
    let mut total = 0u64;
    for (i, px) in bgra.as_chunks::<4>().0.iter().enumerate() {
        total += u64::from(px[2].abs_diff(decoded_rgb[i * 3]));
        total += u64::from(px[1].abs_diff(decoded_rgb[i * 3 + 1]));
        total += u64::from(px[0].abs_diff(decoded_rgb[i * 3 + 2]));
    }
    total as f64 / (bgra.len() / 4 * 3) as f64
}

#[test]
fn decoded_video_matches_the_source() {
    let (w, h) = (320u32, 180u32);
    let mut sender = packetizer();
    let mut viewer = ScreenViewer::new();
    let mut decoder = Decoder::new().expect("decoder");
    let now = Instant::now();

    let mut decoded = 0;
    for t in 0..24 {
        let frame = test_frame(w, h, t);
        let out = sender.push_frame(&frame, t * 33, MAX_DATAGRAM_SIZE).unwrap();
        for assembled in deliver(&mut viewer, out, now) {
            let yuv = decoder.decode(&assembled.data).expect("decodable").expect("a picture");
            assert_eq!(yuv.dimensions(), (w as usize, h as usize));
            let mut rgb = vec![0u8; (w * h * 3) as usize];
            yuv.write_rgb8(&mut rgb);
            let error = mean_abs_error(&rgb, &frame);
            assert!(error < 12.0, "frame {t}: mean error {error:.1}/255 - the picture does not match the source");
            decoded += 1;
        }
    }
    assert_eq!(decoded, 24, "every frame must reach the decoder");
}

#[test]
fn video_survives_a_lost_frame_through_a_keyframe() {
    let (w, h) = (320u32, 180u32);
    let mut sender = packetizer();
    let mut viewer = ScreenViewer::new();
    let mut decoder = Decoder::new().expect("decoder");
    let now = Instant::now();

    let mut decoded_after_recovery = 0;
    for t in 0..20 {
        let frame = test_frame(w, h, t);
        let out = sender.push_frame(&frame, t * 33, MAX_DATAGRAM_SIZE).unwrap();
        if t == 5 {
            continue; // the whole frame is lost on the network
        }
        if viewer.take_keyframe_request(now + Duration::from_secs(u64::from(t))) && t > 5 {
            sender.request_keyframe();
        }
        for assembled in deliver(&mut viewer, out, now) {
            let picture = decoder.decode(&assembled.data).expect("decodable");
            if t > 5 && assembled.is_keyframe {
                decoded_after_recovery += 1;
            }
            if t > 6 && !assembled.is_keyframe {
                assert!(picture.is_some());
                decoded_after_recovery += 1;
            }
        }
    }
    assert!(decoded_after_recovery >= 3, "the stream never recovered after the loss");
}

// ---- frames larger than the encoder supports -------------------------------------------------

#[test]
fn a_frame_beyond_4k_is_scaled_down_keeping_its_shape() {
    let mut p = packetizer();
    let out = p.push_frame(&test_frame(4400, 1800, 0), 0, MAX_DATAGRAM_SIZE).unwrap();
    assert!(is_key(&out));
    let (w, h) = p.dimensions().expect("encoder running");
    assert!(w <= 3840 && h <= 2160, "{w}x{h} is still too big");
    assert!(w % 2 == 0 && h % 2 == 0, "{w}x{h} must be even");
    let (aspect_in, aspect_out) = (4400.0 / 1800.0, f64::from(w) / f64::from(h));
    assert!((aspect_in - aspect_out).abs() < 0.01, "aspect {aspect_out} vs {aspect_in}");
}

#[test]
fn a_scaled_frame_decodes_and_resembles_the_source() {
    let mut p = packetizer();
    let source = test_frame(3900, 2200, 0);
    let OutgoingVideo::KeyFrame { header, data } = p.push_frame(&source, 0, MAX_DATAGRAM_SIZE).unwrap() else {
        panic!("key frame expected")
    };
    let _ = header;
    let (w, h) = p.dimensions().unwrap();
    let mut decoder = Decoder::new().unwrap();
    let yuv = decoder.decode(&data).unwrap().expect("a picture");
    assert_eq!(yuv.dimensions(), (w as usize, h as usize));
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    yuv.write_rgb8(&mut rgb);
    // The picture is a smooth gradient: the centre pixel must match the source's centre.
    let centre = ((h / 2 * w + w / 2) * 3) as usize;
    let src_centre = &source.to_bgra()[((2200 / 2 * 3900 + 3900 / 2) * 4) as usize..][..4];
    let expected = [src_centre[2], src_centre[1], src_centre[0]];
    for c in 0..3 {
        assert!(rgb[centre + c].abs_diff(expected[c]) < 25, "channel {c}: {} vs {}", rgb[centre + c], expected[c]);
    }
}

// ---- bitrate follows the losses viewers report -----------------------------------------------

fn start_rate() -> u32 {
    4_000_000
}

fn warmed_up() -> ScreenPacketizer {
    let mut p = packetizer();
    p.push_frame(&test_frame(320, 180, 0), 0, MAX_DATAGRAM_SIZE).unwrap();
    p
}

#[test]
fn repeated_losses_lower_the_bitrate_and_restart_with_a_keyframe() {
    let mut p = warmed_up();
    assert_eq!(p.bitrate_bps(), Some(start_rate()));
    for at in [1_000, 2_000, 3_000] {
        p.report_loss(at);
    }
    let out = p.push_frame(&test_frame(320, 180, 1), 3_100, MAX_DATAGRAM_SIZE).unwrap();
    assert_eq!(p.bitrate_bps(), Some(start_rate() / 4 * 3));
    assert!(is_key(&out), "the new rate needs a new key frame");
}

#[test]
fn occasional_losses_do_not_change_the_bitrate() {
    let mut p = warmed_up();
    for at in [1_000, 20_000, 40_000, 60_000] {
        p.report_loss(at);
        p.push_frame(&test_frame(320, 180, 1), at + 10, MAX_DATAGRAM_SIZE).unwrap();
    }
    assert_eq!(p.bitrate_bps(), Some(start_rate()));
}

#[test]
fn bitrate_never_drops_below_the_floor() {
    let mut p = warmed_up();
    let mut now = 1_000;
    for _ in 0..40 {
        for _ in 0..3 {
            p.report_loss(now);
            now += 100;
        }
        p.push_frame(&test_frame(320, 180, 1), now, MAX_DATAGRAM_SIZE).unwrap();
        now += 100;
    }
    assert_eq!(p.bitrate_bps(), Some(500_000));
}

#[test]
fn bitrate_recovers_after_a_calm_period_but_not_above_the_start() {
    let mut p = warmed_up();
    for at in [1_000, 2_000, 3_000] {
        p.report_loss(at);
    }
    p.push_frame(&test_frame(320, 180, 1), 3_100, MAX_DATAGRAM_SIZE).unwrap();
    let lowered = p.bitrate_bps().unwrap();
    assert!(lowered < start_rate());

    let mut now = 3_100;
    for _ in 0..8 {
        now += 31_000;
        p.push_frame(&test_frame(320, 180, 2), now, MAX_DATAGRAM_SIZE).unwrap();
    }
    assert_eq!(p.bitrate_bps(), Some(start_rate()));
}
