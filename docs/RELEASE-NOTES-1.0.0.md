FinchSpeak 1.0.0 — self-hosted voice, chat and screen sharing for Windows.

Downloads:
- `FinchSpeak_1.0.0_x64-setup.exe`: client installer.
- `FinchSpeak-server-windows.zip`: server with configuration and launcher.
- `FinchSpeak-windows.zip`: portable client and server for local or two-PC testing.
- `SHA256SUMS.txt`: archive checksums; the installer also has a minisign signature.

Features include text channels, replies, search, reactions, attachments, avatars,
voice rooms, screen sharing with system audio, invitations and moderation,
database backups, tray controls, notifications and signed client updates.

Security: HTTPS/QUIC with certificate fingerprint confirmation; no end-to-end encryption.
The installer has no Authenticode certificate, so Windows may show SmartScreen.

This is the release prepared for the owner's two-PC acceptance. Automated checks
and local measurements are recorded in `docs/RELEASE-VALIDATION.md`.
Real microphone/speaker quality, internet/NAT and clean-Windows installation still
require the checks in `docs/RELEASE-CHECKLIST.md`. Docker has not been validated.

Known limitation: sharing system audio also captures remote participants played
through the same output device. Use headphones and disable system audio sharing
when it would send participants' voices back to them.
