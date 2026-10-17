//! Explicitly pinned, isolated published/new-server compatibility probe.
use dashmap::DashMap;
use finch_audio::VoiceCaptureConfig;
use finch_client_core::api::ApiClient;
use finch_protocol::{
    messages::{ClientMessage, ServerMessage},
    models::ChannelType,
};
use finch_testbed::{
    audio::load_speech_48k,
    client::{TEST_PASSWORD, TestClient},
    voice::run_voice_link_with_config,
};
use std::{sync::Arc, time::Duration};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 4,
        "usage: external_voice https-origin quic-address colon-SHA256"
    );
    let pins = Arc::new(DashMap::new());
    pins.insert("127.0.0.1".to_string(), args[3].clone());
    let mut api = ApiClient::with_tofu(&args[1], pins)?;
    api.verify_trust().await?; // no credentials before certificate verification
    let admin = api
        .register("compatadmin", TEST_PASSWORD, "Compatibility Admin")
        .await?;
    api.set_token(admin.token);
    let invite = api.create_invite(Some(1), Some(1)).await?;
    api.register_with_invite(
        "compatmember",
        TEST_PASSWORD,
        "Compatibility Member",
        &invite.code,
    )
    .await?;
    let mut a = TestClient::connect_external(
        &args[1],
        args[2].parse()?,
        "compatadmin",
        TEST_PASSWORD,
        &args[3],
    )
    .await?;
    let mut b = TestClient::connect_external(
        &args[1],
        args[2].parse()?,
        "compatmember",
        TEST_PASSWORD,
        &args[3],
    )
    .await?;
    let (channel_id, _) = a.channel(ChannelType::Text);
    a.send(ClientMessage::SendMessage {
        channel_id,
        content: "native1.1 compatibility".into(),
        attachments: vec![],
        client_msg_id: None,
        reply_to: None,
    })
    .await?;
    b.expect_control(Duration::from_secs(3), |m| match m {
        ServerMessage::NewMessage { content, .. } if content == "native1.1 compatibility" => {
            Some(())
        }
        _ => None,
    })
    .await?;
    a.join_voice().await?;
    b.join_voice().await?;
    let input = load_speech_48k("speech_1")?;
    for enabled in [true, false] {
        let config = VoiceCaptureConfig {
            voice_enhancement: enabled,
            ..Default::default()
        };
        for run in [
            run_voice_link_with_config(&a, &mut b, &input, 25, config).await?,
            run_voice_link_with_config(&b, &mut a, &input, 25, config).await?,
        ] {
            anyhow::ensure!(
                run.received_headers.len() > 50 && run.output.iter().any(|v| v.abs() > 0.005),
                "no decoded voice"
            );
            anyhow::ensure!(run.max_buffered_packets <= 6, "buffer growth");
            println!(
                "PASS enhancement={enabled} sent={} received={} maxbuffer={}",
                run.sent_packets,
                run.received_headers.len(),
                run.max_buffered_packets
            );
        }
    }
    println!("PASS pinned login, admin/invite/member, chat and two-way native Opus");
    Ok(())
}
