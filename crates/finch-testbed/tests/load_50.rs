//! 50 people at once: one voice room with six speakers, chat, two screen shares with six viewers each.
//!
//! Run on purpose (it takes a minute and uses the machine):
//! `cargo test -p finch-testbed --test load_50 --release -- --ignored --nocapture`
//! Lines starting with `LOAD` go into BASELINE.md.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use finch_client_core::session::SessionEvent;
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::{ChannelType, VideoCodec};
use finch_screen::slice_p_frame;
use finch_testbed::client::{audio_datagram, TestClient};
use finch_testbed::server::TestServer;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use uuid::Uuid;

const CLIENTS: usize = 50;
const SPEAKERS: usize = 6;
const CHATTERS: usize = 5;
const SHARERS: usize = 2;
const VIEWERS_PER_SHARE: usize = 6;
const RUN: Duration = Duration::from_secs(20);

#[derive(Default)]
struct Totals {
    voice_received: AtomicU64,
    video_received: AtomicU64,
    chat_received: AtomicU64,
    lost_connections: AtomicU64,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "heavy: run on purpose"]
async fn fifty_people_voice_chat_and_two_screen_shares() -> anyhow::Result<()> {
    let server = TestServer::start_with(|c| {
        c.auth.register_per_hour = 10_000;
        c.auth.message_limit = 1_000;
    })
    .await?;

    // --- everybody signs in
    let started = Instant::now();
    let mut clients = Vec::with_capacity(CLIENTS);
    for n in 0..CLIENTS {
        clients.push(TestClient::register(&server, &format!("user{n:02}")).await?);
    }
    eprintln!("LOAD sign-in of {CLIENTS} clients: {:.1} s", started.elapsed().as_secs_f64());

    let (text_channel, _) = clients[0].channel(ChannelType::Text);
    let voice_short = clients[0].channel(ChannelType::Voice).1;
    for client in &mut clients {
        client.join_voice().await?;
    }

    // Who does what.
    let totals = Arc::new(Totals::default());
    let chat_latencies: Arc<Mutex<Vec<f64>>> = Arc::default();
    let sharer_peers: Vec<u32> = (0..SHARERS).map(|i| clients[SPEAKERS + i].session.session_peer_id).collect();
    for i in 0..SHARERS {
        let sharer = &clients[SPEAKERS + i];
        sharer.send(ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 1920, height: 1080, fps: 30 }).await?;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    for i in 0..SHARERS {
        let target = clients[SPEAKERS + i].session.user_id;
        for viewer in 0..VIEWERS_PER_SHARE {
            clients[SPEAKERS + SHARERS + i * VIEWERS_PER_SHARE + viewer].send(ClientMessage::SubscribeScreenShare { target_user_id: target }).await?;
        }
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    // --- resource sampler for this whole process (server and all clients live in it)
    let sampler_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let samples: Arc<Mutex<Vec<(f32, u64)>>> = Arc::default();
    let sampler = {
        let (stop, samples) = (sampler_stop.clone(), samples.clone());
        tokio::spawn(async move {
            let mut system = System::new();
            let pid = Pid::from_u32(std::process::id());
            while !stop.load(Ordering::SeqCst) {
                system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::everything());
                if let Some(process) = system.process(pid) {
                    samples.lock().unwrap().push((process.cpu_usage(), process.memory()));
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
    };

    // --- the run
    let until = tokio::time::Instant::now() + RUN;
    let mut tasks = Vec::new();
    let mut sent_voice = 0u64;
    let mut sent_chat = 0u64;
    let mut sent_video_packets = 0u64;
    let sent_voice_counter = Arc::new(AtomicU64::new(0));
    let sent_chat_counter = Arc::new(AtomicU64::new(0));
    let sent_video_counter = Arc::new(AtomicU64::new(0));

    for (index, mut client) in clients.into_iter().enumerate() {
        let totals = totals.clone();
        let latencies = chat_latencies.clone();
        let (voice_sent, chat_sent, video_sent) = (sent_voice_counter.clone(), sent_chat_counter.clone(), sent_video_counter.clone());
        let peer = client.session.session_peer_id;
        let sharer_index = index.checked_sub(SPEAKERS).filter(|i| *i < SHARERS);
        tasks.push(tokio::spawn(async move {
            // Talking, chatting and sharing happen beside the receiving loop.
            let sender: tokio::task::JoinHandle<()> = {
                let connection = client.session.connection.clone();
                let control_tx = client.session.control_tx.clone();
                tokio::spawn(async move {
                    let mut tick = tokio::time::interval(Duration::from_millis(20));
                    let mut seq = 0u16;
                    let mut frame = 0u16;
                    let mut second = 0u32;
                    while tokio::time::Instant::now() < until {
                        tick.tick().await;
                        if index < SPEAKERS {
                            let _ = connection.send_datagram(audio_datagram(voice_short, peer, seq, 80));
                            voice_sent.fetch_add(1, Ordering::Relaxed);
                        }
                        if sharer_index.is_some() && seq.is_multiple_of(2) {
                            // 30 pictures a second, three fragments each (about 3.4 kB).
                            let delta = vec![(frame % 251) as u8; 3300];
                            for packet in slice_p_frame(voice_short, peer, frame.wrapping_add(1), 1, 1033, &delta, false) {
                                let _ = connection.send_datagram(packet.encode());
                                video_sent.fetch_add(1, Ordering::Relaxed);
                            }
                            frame = frame.wrapping_add(1);
                        }
                        seq = seq.wrapping_add(1);
                        second += 1;
                        if index >= CLIENTS - CHATTERS && second.is_multiple_of(50) {
                            let text = format!("load {}", now_ms());
                            let _ = control_tx.try_send(ClientMessage::SendMessage {
                                channel_id: text_channel,
                                content: text,
                                attachments: vec![],
                                client_msg_id: Some(Uuid::new_v4()),
                                reply_to: None,
                            });
                            chat_sent.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                })
            };
            client
                .pump_events(until + Duration::from_secs(1), |event| match event {
                    SessionEvent::Datagram(bytes) if bytes.len() >= MEDIA_HEADER_SIZE => match MediaHeader::from_slice(&bytes[..MEDIA_HEADER_SIZE]).map(|h| h.media_type) {
                        Ok(MediaType::Audio) => { totals.voice_received.fetch_add(1, Ordering::Relaxed); }
                        Ok(_) => { totals.video_received.fetch_add(1, Ordering::Relaxed); }
                        Err(_) => {}
                    },
                    SessionEvent::Control(ServerMessage::NewMessage { content, .. }) => {
                        totals.chat_received.fetch_add(1, Ordering::Relaxed);
                        if index == 0
                            && let Some(sent) = content.strip_prefix("load ").and_then(|t| t.parse::<u128>().ok())
                        {
                            latencies.lock().unwrap().push(now_ms().saturating_sub(sent) as f64);
                        }
                    }
                    SessionEvent::ConnectionLost(_) => { totals.lost_connections.fetch_add(1, Ordering::Relaxed); }
                    _ => {}
                })
                .await;
            let _ = sender.await;
            client
        }));
    }

    let mut survivors = Vec::new();
    for task in tasks {
        survivors.push(task.await?);
    }
    sampler_stop.store(true, Ordering::SeqCst);
    let _ = sampler.await;
    sent_voice += sent_voice_counter.load(Ordering::Relaxed);
    sent_chat += sent_chat_counter.load(Ordering::Relaxed);
    sent_video_packets += sent_video_counter.load(Ordering::Relaxed);

    // --- results
    let listeners = (CLIENTS - 1) as u64;
    let expected_voice = sent_voice * listeners;
    let voice_received = totals.voice_received.load(Ordering::Relaxed);
    let expected_video = sent_video_packets * VIEWERS_PER_SHARE as u64;
    let video_received = totals.video_received.load(Ordering::Relaxed);
    let expected_chat = sent_chat * CLIENTS as u64;
    let chat_received = totals.chat_received.load(Ordering::Relaxed);
    let mut latencies = chat_latencies.lock().unwrap().clone();
    latencies.sort_by(f64::total_cmp);
    let samples = samples.lock().unwrap().clone();
    let cpu_avg = samples.iter().map(|s| s.0).sum::<f32>() / samples.len().max(1) as f32;
    let cpu_max = samples.iter().map(|s| s.0).fold(0.0, f32::max);
    let rss_max = samples.iter().map(|s| s.1).max().unwrap_or(0) as f64 / 1024.0 / 1024.0;

    eprintln!("LOAD voice: sent {sent_voice} (x{listeners} listeners), received {voice_received}/{expected_voice} = {:.2} %", 100.0 * voice_received as f64 / expected_voice.max(1) as f64);
    eprintln!("LOAD video: sent {sent_video_packets} packets (x{VIEWERS_PER_SHARE} viewers), received {video_received}/{expected_video} = {:.2} %", 100.0 * video_received as f64 / expected_video.max(1) as f64);
    eprintln!("LOAD chat: sent {sent_chat}, received {chat_received}/{expected_chat}; latency p50/p95/max = {:.0}/{:.0}/{:.0} ms ({} samples)", percentile(&latencies, 0.5), percentile(&latencies, 0.95), percentile(&latencies, 1.0), latencies.len());
    eprintln!("LOAD process (server + 50 clients together, 8 worker threads): CPU avg {cpu_avg:.0} % / max {cpu_max:.0} % of one core, RSS max {rss_max:.0} MB");
    eprintln!("LOAD connections lost: {}", totals.lost_connections.load(Ordering::Relaxed));
    let _ = (sharer_peers, survivors.len());

    assert_eq!(totals.lost_connections.load(Ordering::Relaxed), 0, "nobody may be dropped");
    assert!(voice_received as f64 >= 0.97 * expected_voice as f64, "voice delivery below 97 %");
    assert!(video_received as f64 >= 0.90 * expected_video as f64, "video delivery below 90 %");
    assert!(chat_received as f64 >= 0.99 * expected_chat as f64, "chat delivery below 99 %");
    assert!(percentile(&latencies, 0.95) < 500.0, "chat p95 above 500 ms");
    Ok(())
}
