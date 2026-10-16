FinchSpeak for Windows
======================

SERVER (one computer, always on)
  1. Run_Server.bat. Open UDP 4433 and TCP 8443 in the firewall / router.
  2. The first account created becomes the admin. Settings: finch-server.toml.
  3. Data lives in the "data" folder and finchspeak.db next to the server:
       data\backups   daily copies of the database (last 7)
       data\logs      daily log files (last 14)
       data\uploads   files people attached
       data\tls       the server certificate - keep it, clients remember it
  Back up the whole folder to move or restore the server.

CLIENT (everyone)
  1. Install FinchSpeak_1.0.0_x64-setup.exe from GitHub Releases, or use
     Run_FinchSpeak.bat from FinchSpeak-windows.zip (the server-only ZIP has no client).
  2. Server address: https://<server address>:8443
  3. The first time, the app shows the server's certificate fingerprint. Compare it with
     the one the admin sees in the server window and confirm.
  4. Registering needs an invite code: the admin creates one in the app (tool icon,
     Administration).

Updating: the installed client checks GitHub Releases and verifies the update signature.
You can also check in Settings > About. Server updates are manual: stop the server,
back up the whole folder, replace finch-server.exe, then start it again. Keep the
database, data folder and finch-server.toml. Never replace data with an empty folder.

The installer is not Authenticode-signed. Windows SmartScreen may show an unknown
publisher warning; verify the download source and SHA256SUMS.txt before proceeding.

Two-PC acceptance: docs/RELEASE-CHECKLIST.md in the repository. Internet/NAT,
microphone/speaker quality and clean-Windows installation require human verification.
