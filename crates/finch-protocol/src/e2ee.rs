//! End-to-End Encryption (E2EE) for media datagrams using ChaCha20-Poly1305.
//!
//! According to Section 10.2 of PLAN.md:
//! - 256-bit symmetric Room Key (`[u8; 32]`).
//! - 12-byte Nonce derived from [`MediaHeader::e2ee_nonce()`].
//! - 20-byte AAD (Additional Authenticated Data) from [`MediaHeader::to_bytes()`].
//! - Payload encrypted with ChaCha20-Poly1305, appending a 16-byte Poly1305 authentication tag.
//! - The `FLAG_E2EE` bit (`0x02`) in `MediaHeader::flags` indicates encryption.

use bytes::Bytes;
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};

use crate::media::{MediaHeader, FLAG_E2EE};

/// Size in bytes of a 256-bit ChaCha20-Poly1305 symmetric key.
pub const E2EE_KEY_SIZE: usize = 32;

/// Size in bytes of a ChaCha20-Poly1305 nonce (12 bytes / 96 bits).
pub const E2EE_NONCE_SIZE: usize = 12;

/// Size in bytes of a Poly1305 authentication tag (16 bytes / 128 bits).
pub const E2EE_TAG_SIZE: usize = 16;

/// Errors that can occur during E2EE encryption or decryption.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum E2eeError {
    /// The media header does not have the `FLAG_E2EE` bit set.
    #[error("Datagram header is missing the E2EE flag (FLAG_E2EE)")]
    MissingE2eeFlag,

    /// Decryption or Poly1305 tag verification failed.
    #[error("Payload decryption or Poly1305 authentication failed")]
    DecryptionFailed,

    /// Encryption failed.
    #[error("Payload encryption failed")]
    EncryptionFailed,
}

/// Encrypt a media datagram payload using ChaCha20-Poly1305.
///
/// Sets the `FLAG_E2EE` flag in `header.flags`, derives the 12-byte nonce from
/// `header.e2ee_nonce()`, uses the 20-byte serialized header (`header.to_bytes()`)
/// as Additional Authenticated Data (AAD), encrypts the plaintext, and appends
/// the 16-byte Poly1305 authentication tag.
pub fn encrypt_media_payload(
    header: &mut MediaHeader,
    plaintext: &[u8],
    key: &[u8; E2EE_KEY_SIZE],
) -> Result<Bytes, E2eeError> {
    header.flags |= FLAG_E2EE;

    let nonce_bytes = header.e2ee_nonce();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let aad = header.to_bytes();

    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let payload = Payload {
        msg: plaintext,
        aad: &aad,
    };

    let ciphertext = cipher
        .encrypt(nonce, payload)
        .map_err(|_| E2eeError::EncryptionFailed)?;

    Ok(Bytes::from(ciphertext))
}

/// Decrypt an end-to-end encrypted media datagram payload using ChaCha20-Poly1305.
///
/// Verifies that `header.flags` has `FLAG_E2EE` set, derives the 12-byte nonce
/// from `header.e2ee_nonce()`, authenticates against the 20-byte header AAD,
/// verifies the Poly1305 tag, and returns the decrypted plaintext bytes.
pub fn decrypt_media_payload(
    header: &MediaHeader,
    ciphertext_with_tag: &[u8],
    key: &[u8; E2EE_KEY_SIZE],
) -> Result<Bytes, E2eeError> {
    if !header.is_e2ee() {
        return Err(E2eeError::MissingE2eeFlag);
    }

    let nonce_bytes = header.e2ee_nonce();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let aad = header.to_bytes();

    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let payload = Payload {
        msg: ciphertext_with_tag,
        aad: &aad,
    };

    let plaintext = cipher
        .decrypt(nonce, payload)
        .map_err(|_| E2eeError::DecryptionFailed)?;

    Ok(Bytes::from(plaintext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::MediaType;

    const TEST_KEY_1: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20,
    ];

    const TEST_KEY_2: [u8; 32] = [
        0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8,
        0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF, 0xB0,
        0xB1, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8,
        0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, 0xC0,
    ];

    #[test]
    fn test_audio_roundtrip() -> Result<(), E2eeError> {
        let mut header = MediaHeader::new_audio(
            42,
            1001,
            1,
            960,
            false,
            false, // is_e2ee starts false; encrypt_media_payload will set it
        );
        let audio_payload = b"test opus encoded audio frame 20ms";

        let ciphertext = encrypt_media_payload(&mut header, audio_payload, &TEST_KEY_1)?;
        assert!(header.is_e2ee());
        assert_eq!(ciphertext.len(), audio_payload.len() + E2EE_TAG_SIZE);

        let decrypted = decrypt_media_payload(&header, &ciphertext, &TEST_KEY_1)?;
        assert_eq!(decrypted.as_ref(), audio_payload);

        Ok(())
    }

    #[test]
    fn test_video_key_and_delta_roundtrip() -> Result<(), E2eeError> {
        // Video Keyframe
        let mut key_header = MediaHeader::new_video_key(
            10,
            2002,
            100,
            90_000,
            100,
            false,
        );
        let key_nal_unit = b"\x00\x00\x00\x01\x65IDR_KEYFRAME_TEST_PAYLOAD";

        let key_ciphertext = encrypt_media_payload(&mut key_header, key_nal_unit, &TEST_KEY_1)?;
        assert!(key_header.is_e2ee());
        assert_eq!(key_ciphertext.len(), key_nal_unit.len() + E2EE_TAG_SIZE);

        let key_decrypted = decrypt_media_payload(&key_header, &key_ciphertext, &TEST_KEY_1)?;
        assert_eq!(key_decrypted.as_ref(), key_nal_unit);

        // Video Delta frame fragment
        let mut delta_header = MediaHeader::new_video_delta(
            10,
            2002,
            101,
            93_000,
            101,
            0,
            3,
            false,
        );
        let delta_nal_fragment = b"\x00\x00\x00\x01\x41DELTA_FRAME_FRAGMENT_TEST_PAYLOAD";

        let delta_ciphertext = encrypt_media_payload(&mut delta_header, delta_nal_fragment, &TEST_KEY_1)?;
        assert!(delta_header.is_e2ee());
        assert_eq!(delta_ciphertext.len(), delta_nal_fragment.len() + E2EE_TAG_SIZE);

        let delta_decrypted = decrypt_media_payload(&delta_header, &delta_ciphertext, &TEST_KEY_1)?;
        assert_eq!(delta_decrypted.as_ref(), delta_nal_fragment);

        Ok(())
    }

    #[test]
    fn test_missing_e2ee_flag_returns_error() -> Result<(), E2eeError> {
        let mut header = MediaHeader::new_audio(1, 2, 3, 4, false, false);
        let payload = b"secret audio";

        let ciphertext = encrypt_media_payload(&mut header, payload, &TEST_KEY_1)?;

        // Manually strip the E2EE flag
        header.flags &= !FLAG_E2EE;
        assert!(!header.is_e2ee());

        let res = decrypt_media_payload(&header, &ciphertext, &TEST_KEY_1);
        assert_eq!(res, Err(E2eeError::MissingE2eeFlag));

        Ok(())
    }

    #[test]
    fn test_wrong_key_fails_decryption() -> Result<(), E2eeError> {
        let mut header = MediaHeader::new_audio(1, 2, 3, 4, false, false);
        let payload = b"confidential voice stream";

        let ciphertext = encrypt_media_payload(&mut header, payload, &TEST_KEY_1)?;

        let res = decrypt_media_payload(&header, &ciphertext, &TEST_KEY_2);
        assert_eq!(res, Err(E2eeError::DecryptionFailed));

        Ok(())
    }

    #[test]
    fn test_tampered_header_aad_fails_decryption() -> Result<(), E2eeError> {
        let mut header = MediaHeader::new_audio(10, 100, 1, 960, false, false);
        let payload = b"voice data";

        let ciphertext = encrypt_media_payload(&mut header, payload, &TEST_KEY_1)?;

        // Tamper with channel_id in AAD
        let mut tampered_header = header;
        tampered_header.channel_id = 999;
        assert_eq!(
            decrypt_media_payload(&tampered_header, &ciphertext, &TEST_KEY_1),
            Err(E2eeError::DecryptionFailed)
        );

        // Tamper with sender_peer_id in AAD and Nonce
        let mut tampered_header = header;
        tampered_header.sender_peer_id = 999;
        assert_eq!(
            decrypt_media_payload(&tampered_header, &ciphertext, &TEST_KEY_1),
            Err(E2eeError::DecryptionFailed)
        );

        // Tamper with sequence
        let mut tampered_header = header;
        tampered_header.sequence = 999;
        assert_eq!(
            decrypt_media_payload(&tampered_header, &ciphertext, &TEST_KEY_1),
            Err(E2eeError::DecryptionFailed)
        );

        // Tamper with timestamp
        let mut tampered_header = header;
        tampered_header.timestamp = 999;
        assert_eq!(
            decrypt_media_payload(&tampered_header, &ciphertext, &TEST_KEY_1),
            Err(E2eeError::DecryptionFailed)
        );

        // Tamper with media_type
        let mut tampered_header = header;
        tampered_header.media_type = MediaType::ScreenAudio;
        assert_eq!(
            decrypt_media_payload(&tampered_header, &ciphertext, &TEST_KEY_1),
            Err(E2eeError::DecryptionFailed)
        );

        Ok(())
    }

    #[test]
    fn test_corrupted_ciphertext_fails_decryption() -> Result<(), E2eeError> {
        let mut header = MediaHeader::new_audio(1, 2, 3, 4, false, false);
        let payload = b"audio message";

        let ciphertext = encrypt_media_payload(&mut header, payload, &TEST_KEY_1)?;

        // Corrupt first byte of payload
        let mut corrupted = ciphertext.to_vec();
        corrupted[0] ^= 0x55;
        assert_eq!(
            decrypt_media_payload(&header, &corrupted, &TEST_KEY_1),
            Err(E2eeError::DecryptionFailed)
        );

        // Corrupt last byte (part of Poly1305 tag)
        let mut corrupted_tag = ciphertext.to_vec();
        let last_idx = corrupted_tag.len() - 1;
        corrupted_tag[last_idx] ^= 0xAA;
        assert_eq!(
            decrypt_media_payload(&header, &corrupted_tag, &TEST_KEY_1),
            Err(E2eeError::DecryptionFailed)
        );

        Ok(())
    }

    #[test]
    fn test_truncated_ciphertext_fails_decryption() -> Result<(), E2eeError> {
        let mut header = MediaHeader::new_audio(1, 2, 3, 4, false, false);
        let payload = b"short";

        let ciphertext = encrypt_media_payload(&mut header, payload, &TEST_KEY_1)?;

        // Truncate to less than tag size
        let truncated = &ciphertext[..10];
        assert_eq!(
            decrypt_media_payload(&header, truncated, &TEST_KEY_1),
            Err(E2eeError::DecryptionFailed)
        );

        Ok(())
    }

    #[test]
    fn test_empty_payload_roundtrip() -> Result<(), E2eeError> {
        let mut header = MediaHeader::new_audio(1, 2, 3, 4, false, false);
        let empty_payload = b"";

        let ciphertext = encrypt_media_payload(&mut header, empty_payload, &TEST_KEY_1)?;
        assert_eq!(ciphertext.len(), E2EE_TAG_SIZE);

        let decrypted = decrypt_media_payload(&header, &ciphertext, &TEST_KEY_1)?;
        assert!(decrypted.is_empty());

        Ok(())
    }

    #[test]
    fn test_sfu_sender_peer_id_authentication() -> Result<(), E2eeError> {
        // Authenticated client peer ID is 1234.
        let auth_peer_id = 1234u32;
        let mut header = MediaHeader::new_audio(1, auth_peer_id, 10, 9600, false, false);
        let payload = b"legitimate user speech";

        let ciphertext = encrypt_media_payload(&mut header, payload, &TEST_KEY_1)?;

        // SFU overwrites sender_peer_id with verified auth_peer_id (matching the client)
        let mut wire_datagram = header.to_bytes().to_vec();
        wire_datagram.extend_from_slice(&ciphertext);
        assert!(MediaHeader::overwrite_sender_peer_id(&mut wire_datagram, auth_peer_id).is_ok());

        // Receiver receives datagram and parses header
        let parsed_header = MediaHeader::from_slice(&wire_datagram[..20])
            .map_err(|_| E2eeError::DecryptionFailed)?;
        let decrypted = decrypt_media_payload(&parsed_header, &wire_datagram[20..], &TEST_KEY_1)?;
        assert_eq!(decrypted.as_ref(), payload);

        // Attack scenario: client forged its sender_peer_id as 9999 when encrypting,
        // but SFU rewrote it to the actual verified session ID 1234.
        let mut spoofed_header = MediaHeader::new_audio(1, 9999, 10, 9600, false, false);
        let spoofed_ciphertext = encrypt_media_payload(&mut spoofed_header, payload, &TEST_KEY_1)?;

        let mut spoofed_wire = spoofed_header.to_bytes().to_vec();
        spoofed_wire.extend_from_slice(&spoofed_ciphertext);
        // SFU enforces the authentic ID 1234
        assert!(MediaHeader::overwrite_sender_peer_id(&mut spoofed_wire, auth_peer_id).is_ok());

        let receiver_header = MediaHeader::from_slice(&spoofed_wire[..20])
            .map_err(|_| E2eeError::DecryptionFailed)?;
        // Receiver decryption MUST fail because AAD and Nonce mismatch what was signed!
        let spoof_res = decrypt_media_payload(&receiver_header, &spoofed_wire[20..], &TEST_KEY_1);
        assert_eq!(spoof_res, Err(E2eeError::DecryptionFailed));

        Ok(())
    }
}
