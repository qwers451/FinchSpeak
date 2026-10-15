use std::time::Duration;

use finch_testbed::netem::{NetemConfig, UdpImpairmentProxy};
use tokio::net::UdpSocket;
use tokio::time::{sleep, timeout};

// A connected UDP socket receives an ICMP port-unreachable error after sending
// to a closed upstream port (WSAECONNRESET on Windows). That must not stop the
// proxy's receive loop permanently.
#[tokio::test]
async fn proxy_recovers_bidirectional_traffic_after_upstream_restarts() {
    finch_testbed::init_tracing();
    let upstream = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let proxy = UdpImpairmentProxy::start(
        upstream_addr,
        NetemConfig {
            loss: 0.0,
            delay_ms: (0, 0),
            seed: 10,
        },
    )
    .await
    .unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.connect(proxy.listen_addr).await.unwrap();

    exchange(&client, &upstream, b"before outage", b"initial reply").await;
    drop(upstream);

    // Multiple sends give the OS time to deliver the asynchronous ICMP error
    // to the real proxy socket while its upstream receive is pending.
    for _ in 0..5 {
        client.send(b"upstream is unavailable").await.unwrap();
        sleep(Duration::from_millis(100)).await;
    }

    let restarted = UdpSocket::bind(upstream_addr).await.unwrap();
    exchange(&client, &restarted, b"after restart", b"recovered reply").await;
    exchange(&client, &restarted, b"still forwarding", b"second reply").await;
}

async fn exchange(client: &UdpSocket, upstream: &UdpSocket, request: &[u8], reply: &[u8]) {
    timeout(Duration::from_secs(3), async {
        client.send(request).await.unwrap();
        let mut buf = [0; 128];
        let (n, proxy_addr) = upstream.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], request, "proxy must forward the actual request");
        upstream.send_to(reply, proxy_addr).await.unwrap();
        let n = client.recv(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], reply, "proxy must forward the actual reply");
    })
    .await
    .expect("proxy did not forward bidirectional traffic");
}
