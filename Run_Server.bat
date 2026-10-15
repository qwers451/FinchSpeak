@echo off
title FinchSpeak Server (QUIC SFU + REST API)
cd /d "%~dp0"
echo ==========================================
echo   Starting FinchSpeak SFU Server...
echo   QUIC: udp://0.0.0.0:4433
echo   REST: https://0.0.0.0:8443 (TLS only)
echo ==========================================
cargo run -p finch-server
pause
