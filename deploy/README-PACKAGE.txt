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
  1. Run_FinchSpeak.bat.
  2. Server address: https://<server address>:8443
  3. The first time, the app shows the server's certificate fingerprint. Compare it with
     the one the admin sees in the server window and confirm.
  4. Registering needs an invite code: the admin creates one in the app (tool icon,
     Administration).

Updating: replace the .exe files with the new ones; the data folder stays.
There is no automatic updater.
