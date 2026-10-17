//! Verify an actual release installer with the public key built into the client.
//! cargo run -p finch-app --example verify-release -- <installer.exe> <latest.json>
use std::{fs, io, path::PathBuf};

use finch_app_lib::updater;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let installer = PathBuf::from(arguments.next().ok_or_else(|| io::Error::other("installer path required"))?);
    let manifest_path = arguments.next().ok_or_else(|| io::Error::other("manifest path required"))?;
    let manifest: updater::Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    if manifest.version != env!("CARGO_PKG_VERSION") {
        return Err(io::Error::other("manifest version does not match the client").into());
    }
    let entry = manifest.platforms.get(updater::PLATFORM).ok_or_else(|| io::Error::other("Windows platform missing"))?;
    if !updater::download_allowed(&entry.url) {
        return Err(io::Error::other("installer download URL is not allowed").into());
    }
    let expected_suffix = format!("/v{0}/FinchSpeak_{0}_x64-setup.exe", manifest.version);
    if !entry.url.ends_with(&expected_suffix) {
        return Err(io::Error::other("installer URL does not match release version/name").into());
    }
    if updater::offer("1.0.1", &manifest).is_none()
        || updater::offer(env!("CARGO_PKG_VERSION"), &manifest).is_some()
        || updater::offer("999.0.0", &manifest).is_some() {
        return Err(io::Error::other("actual manifest update offer/downgrade policy failed").into());
    }
    let signature_path = PathBuf::from(format!("{}.sig", installer.display()));
    if fs::read_to_string(signature_path)?.trim() != entry.signature.trim() {
        return Err(io::Error::other("manifest signature differs from installer signature").into());
    }
    let bytes = fs::read(installer)?;
    updater::verify(&bytes, &entry.signature, updater::PUBLIC_KEY_BASE64).map_err(io::Error::other)?;
    let mut tampered = bytes;
    if let Some(byte) = tampered.first_mut() {
        *byte ^= 1;
    }
    if updater::verify(&tampered, &entry.signature, updater::PUBLIC_KEY_BASE64).is_ok() {
        return Err(io::Error::other("tampered installer was accepted").into());
    }
    println!("PASS: release manifest matches client version; installer signature verified with built-in public key; modified installer rejected; old-client offer/equal-or-newer suppression verified");
    Ok(())
}
