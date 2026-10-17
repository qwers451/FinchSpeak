//! Both production capture modes through a real SFU, plus strict external TLS pinning.
use finch_audio::VoiceCaptureConfig;
use finch_testbed::{
    audio::load_speech_48k,
    client::{TEST_PASSWORD, TestClient},
    server::TestServer,
    voice::run_voice_link_with_config,
};
#[tokio::test]
async fn external_endpoint_rejects_wrong_pin_before_login() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let result = TestClient::connect_external(
        &server.http_url,
        server.quic_addr,
        "never_registered",
        TEST_PASSWORD,
        &"00".repeat(32),
    )
    .await;
    let error = result.err().expect("wrong pin must fail").to_string();
    assert!(
        error.contains("Certificate") && error.contains("changed"),
        "{error}"
    );
    let _registered = TestClient::register(&server, "strict_valid").await?;
    let client = TestClient::connect_external(
        &server.http_url,
        server.quic_addr,
        "strict_valid",
        TEST_PASSWORD,
        &server.certificate_fingerprint,
    )
    .await?;
    assert_eq!(client.user.username, "strict_valid");
    Ok(())
}
#[tokio::test]
async fn full_capture_on_off_survives_two_way_voice_and_reconnect() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "native_a").await?;
    let mut b = TestClient::register(&server, "native_b").await?;
    a.join_voice().await?;
    b.join_voice().await?;
    let input = load_speech_48k("speech_2")?;
    for enabled in [true, false] {
        let config = VoiceCaptureConfig {
            voice_enhancement: enabled,
            ..Default::default()
        };
        for run in [
            run_voice_link_with_config(&a, &mut b, &input, 25, config).await?,
            run_voice_link_with_config(&b, &mut a, &input, 25, config).await?,
        ] {
            assert!(run.sent_packets > 50 && run.received_headers.len() > 50);
            assert!(run.max_buffered_packets <= 6);
            assert!(run.output.iter().all(|s| s.is_finite()));
            assert!(run.output.iter().any(|s| s.abs() > 0.005));
        }
    }
    a.reconnect(&server).await?;
    a.join_voice().await?;
    let run =
        run_voice_link_with_config(&a, &mut b, &input, 25, VoiceCaptureConfig::default()).await?;
    assert!(run.received_headers.len() > 50);
    Ok(())
}
