# FinchSpeak 1.0.1

Windows server startup and release validation fixes:

- The server includes its Visual C++ runtime statically. No separate VC++
  Redistributable installation is required for the server.
- Startup failures after configuration and logging initialize, including occupied
  TCP/UDP ports, now retain their complete error chain in `data/logs`. Invalid
  configuration files are reported in the console. The console announces a ready server,
  listening addresses and certificate fingerprint, even with `RUST_LOG=warn`.
- `Run_Server.bat` detects incomplete extraction, explains what to do and preserves
  the server's exit code. Extract all ZIP contents into a writable folder first.
- A clean first start no longer prints a false log-directory error before creating
  the logging folder.
- Release checks exercise the real BAT with shipped settings in both ZIPs,
  Cyrillic/spaces in paths, restart, busy ports, invalid settings/TLS, missing files,
  API bootstrap, invitations and database backup/restore. PE imports are checked
  both before packaging and after extraction to prevent runtime dependency regressions.

Updating an existing server: stop it and back up the entire folder. Replace only
`finch-server.exe`, `Run_Server.bat` and `README.txt` from the new server archive.
Keep `finch-server.toml`, `finchspeak.db` and the entire `data` directory.

The client version is 1.0.1 so the installer and signed updater manifest stay in
sync with the release. There are no client protocol or database schema changes.
The installer is not Authenticode-signed. Two-PC internet/NAT and real audio
quality acceptance remain necessary; local automated checks cannot establish them.
