//! Updates from GitHub Releases: a `latest.json` next to the installer, the installer signed with a
//! minisign key whose public half is built into the app. Nothing is run unless the signature fits.

use base64::Engine;
use serde::{Deserialize, Serialize};

/// The key every update must be signed with (its private half is kept by the owner, never in the repository).
pub const PUBLIC_KEY_BASE64: &str = include_str!("update_key.pub");
/// Where the manifest of the newest release lives.
pub const MANIFEST_URL: &str = "https://github.com/qwers451/FinchSpeak/releases/latest/download/latest.json";
/// The platform key inside the manifest.
pub const PLATFORM: &str = "windows-x86_64";
/// Installers and manifests larger than this are refused.
pub const MAX_INSTALLER_BYTES: usize = 200 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub platforms: std::collections::HashMap<String, PlatformEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformEntry {
    /// Base64 of the minisign signature file, as `tauri signer sign` prints it.
    pub signature: String,
    pub url: String,
}

/// What the interface is told about an available update.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
}

/// `1.2.3`, `v1.2.3` or `1.2.3-rc.1` as comparable numbers (a pre-release is older than its release).
fn parse_version(text: &str) -> Option<(u64, u64, u64, bool)> {
    let text = text.trim().trim_start_matches('v');
    let (numbers, pre) = match text.split_once('-') {
        Some((numbers, _)) => (numbers, true),
        None => (text, false),
    };
    let mut parts = numbers.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch, pre))
}

/// Is `candidate` newer than `current`? Anything that cannot be read counts as not newer.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (parse_version(current), parse_version(candidate)) {
        (Some((a, b, c, current_pre)), Some((x, y, z, candidate_pre))) => {
            (x, y, z, !candidate_pre) > (a, b, c, !current_pre)
        }
        _ => false,
    }
}

/// Only addresses on GitHub are followed for the installer (the signature protects the content;
/// this stops the manifest from pointing anywhere else).
pub fn download_allowed(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else { return false };
    parsed.scheme() == "https" && matches!(parsed.host_str(), Some("github.com") | Some("objects.githubusercontent.com") | Some("release-assets.githubusercontent.com"))
}

/// Does `signature_base64` (the content of the `.sig` file, base64) sign `data` with `public_key_base64`?
pub fn verify(data: &[u8], signature_base64: &str, public_key_base64: &str) -> Result<(), String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let decode = |text: &str| -> Result<String, String> {
        let bytes = engine.decode(text.trim()).map_err(|_| "The update signature is not valid text".to_string())?;
        String::from_utf8(bytes).map_err(|_| "The update signature is not valid text".to_string())
    };
    let key = minisign_verify::PublicKey::decode(&decode(public_key_base64)?).map_err(|e| format!("The built-in update key is broken: {e}"))?;
    let signature = minisign_verify::Signature::decode(&decode(signature_base64)?).map_err(|e| format!("The update signature is broken: {e}"))?;
    key.verify(data, &signature, false).map_err(|_| "The update is not signed by FinchSpeak and was not installed".to_string())
}

/// The update to offer, if the manifest has a newer one for this platform.
pub fn offer(current: &str, manifest: &Manifest) -> Option<(UpdateInfo, PlatformEntry)> {
    if !is_newer(current, &manifest.version) {
        return None;
    }
    let entry = manifest.platforms.get(PLATFORM)?.clone();
    Some((UpdateInfo { version: manifest.version.trim_start_matches('v').to_string(), notes: manifest.notes.clone() }, entry))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD: &[u8] = include_bytes!("fixtures/payload.bin");
    const SIGNATURE: &str = include_str!("fixtures/payload.bin.sig");
    const TEST_KEY: &str = include_str!("fixtures/test.pub");

    #[test]
    fn newer_versions_are_found_and_older_or_equal_are_not() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(is_newer("1.2.3", "v1.10.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0.1", "1.0.0"));
        assert!(!is_newer("2.0.0", "1.99.99"));
    }

    #[test]
    fn a_release_beats_its_own_pre_release_but_not_the_other_way_round() {
        assert!(is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0-rc.1"));
        assert!(is_newer("0.9.0", "1.0.0-rc.1"));
    }

    #[test]
    fn rubbish_versions_are_never_offered() {
        for bad in ["", "abc", "1.x.0", "1.2.3.4", "-", "99999999999999999999.0.0"] {
            assert!(!is_newer("1.0.0", bad), "{bad:?}");
            assert!(!is_newer(bad, "1.0.0"), "{bad:?}");
        }
    }

    #[test]
    fn a_genuine_signature_passes() {
        verify(PAYLOAD, SIGNATURE, TEST_KEY).unwrap();
    }

    #[test]
    fn a_changed_file_a_foreign_key_or_garbage_fail() {
        let mut changed = PAYLOAD.to_vec();
        changed[0] ^= 1;
        assert!(verify(&changed, SIGNATURE, TEST_KEY).is_err(), "tampered installer");
        assert!(verify(PAYLOAD, SIGNATURE, PUBLIC_KEY_BASE64).is_err(), "signed by somebody else");
        assert!(verify(PAYLOAD, "not base64!!", TEST_KEY).is_err());
        assert!(verify(PAYLOAD, "", TEST_KEY).is_err());
        assert!(verify(PAYLOAD, SIGNATURE, "AAAA").is_err());
    }

    #[test]
    fn the_built_in_key_is_a_valid_key() {
        // A broken key would make every update fail with a confusing message.
        let engine = base64::engine::general_purpose::STANDARD;
        let text = String::from_utf8(engine.decode(PUBLIC_KEY_BASE64.trim()).unwrap()).unwrap();
        minisign_verify::PublicKey::decode(&text).unwrap();
    }

    fn manifest(version: &str, with_platform: bool) -> Manifest {
        let mut platforms = std::collections::HashMap::new();
        if with_platform {
            platforms.insert(PLATFORM.to_string(), PlatformEntry { signature: "c2ln".into(), url: "https://github.com/x/y/releases/download/v/app.exe".into() });
        }
        Manifest { version: version.into(), notes: "Fixes".into(), platforms }
    }

    #[test]
    fn an_update_is_offered_only_when_newer_and_built_for_this_platform() {
        assert_eq!(offer("0.1.0", &manifest("v0.2.0", true)).map(|(info, _)| info), Some(UpdateInfo { version: "0.2.0".into(), notes: "Fixes".into() }));
        assert!(offer("0.2.0", &manifest("0.2.0", true)).is_none());
        assert!(offer("0.1.0", &manifest("0.2.0", false)).is_none());
    }

    #[test]
    fn installers_may_only_come_from_github() {
        assert!(download_allowed("https://github.com/qwers451/FinchSpeak/releases/download/v1/app.exe"));
        assert!(download_allowed("https://objects.githubusercontent.com/abc"));
        for bad in ["http://github.com/x", "https://evil.example/app.exe", "https://github.com.evil.example/x", "file:///C:/x.exe", "ftp://github.com/x", "not a url", "https://github.com@evil.example/x"] {
            assert!(!download_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn a_manifest_in_the_tauri_format_is_read() {
        let json = r#"{"version":"1.0.1","notes":"n","pub_date":"2026-01-01T00:00:00Z","platforms":{"windows-x86_64":{"signature":"abc","url":"https://github.com/a/b/c.exe"}}}"#;
        let parsed: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.platforms[PLATFORM].url, "https://github.com/a/b/c.exe");
    }
}
