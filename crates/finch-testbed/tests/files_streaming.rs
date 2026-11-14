//! Attachments are streamed to and from disk; each user has a storage quota and an upload rate.

use std::hash::{Hash, Hasher};
use std::path::Path;

use finch_client_core::error::ClientError;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

fn status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

fn hash_of(data: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

/// Names of the files left in the upload directory.
fn stored_files(dir: &Path) -> Vec<String> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// A pseudo-random body, so a mix-up of chunks cannot go unnoticed.
fn noise(len: usize) -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

#[tokio::test]
async fn a_40_mb_file_goes_up_and_comes_back_intact() -> anyhow::Result<()> {
    let server = TestServer::start_with(|c| c.storage.max_file_size_mb = 50).await?;
    let a = TestClient::register(&server, "alice").await?;
    let data = noise(40 * 1024 * 1024);
    let info = a.api.upload_file("big.bin", data.clone(), "application/octet-stream").await?;
    assert_eq!(info.size_bytes, data.len() as u64);
    let back = a.api.download_file(info.id).await?;
    assert_eq!(back.len(), data.len());
    assert_eq!(hash_of(&back), hash_of(&data));
    Ok(())
}

#[tokio::test]
async fn an_oversized_upload_is_refused_and_leaves_nothing_on_disk() -> anyhow::Result<()> {
    let uploads = tempfile::tempdir()?;
    let dir = uploads.path().to_string_lossy().to_string();
    let server = TestServer::start_with(move |c| {
        c.storage.max_file_size_mb = 1;
        c.storage.upload_dir = dir;
    })
    .await?;
    let a = TestClient::register(&server, "alice").await?;
    let refused = a.api.upload_file("big.bin", noise(3 * 1024 * 1024), "application/octet-stream").await.expect_err("3 MB");
    assert_eq!(status(&refused), Some(413));
    assert_eq!(stored_files(uploads.path()), Vec::<String>::new(), "the half-written file is deleted");
    Ok(())
}

#[tokio::test]
async fn the_per_user_quota_stops_further_uploads() -> anyhow::Result<()> {
    let uploads = tempfile::tempdir()?;
    let dir = uploads.path().to_string_lossy().to_string();
    let server = TestServer::start_with(move |c| {
        c.storage.max_file_size_mb = 5;
        c.storage.user_quota_mb = 1;
        c.storage.upload_dir = dir;
    })
    .await?;
    let a = TestClient::register(&server, "alice").await?;
    let b = TestClient::register(&server, "bob").await?;

    a.api.upload_file("one.bin", noise(600 * 1024), "application/octet-stream").await?;
    let refused = a.api.upload_file("two.bin", noise(600 * 1024), "application/octet-stream").await.expect_err("over quota");
    assert_eq!(status(&refused), Some(413));
    match &refused {
        ClientError::Http { body, .. } => assert!(body.contains("Storage quota exceeded"), "{body}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(stored_files(uploads.path()).len(), 1, "only the first file is kept");

    // The quota is per user.
    b.api.upload_file("bob.bin", noise(600 * 1024), "application/octet-stream").await?;
    Ok(())
}

#[tokio::test]
async fn uploads_per_minute_are_limited() -> anyhow::Result<()> {
    let server = TestServer::start_with(|c| c.storage.uploads_per_minute = 3).await?;
    let a = TestClient::register(&server, "alice").await?;
    let b = TestClient::register(&server, "bob").await?;
    for n in 0..3 {
        a.api.upload_file(&format!("{n}.txt"), b"hello".to_vec(), "text/plain").await?;
    }
    let refused = a.api.upload_file("4.txt", b"hello".to_vec(), "text/plain").await.expect_err("fourth upload");
    assert_eq!(status(&refused), Some(429));
    match &refused {
        ClientError::Http { body, .. } => assert!(body.contains("Too many uploads"), "{body}"),
        other => panic!("{other:?}"),
    }
    b.api.upload_file("bob.txt", b"hello".to_vec(), "text/plain").await?;
    Ok(())
}
