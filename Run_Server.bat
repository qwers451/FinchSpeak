@echo off
title FinchSpeak Server (QUIC SFU + REST API)
cd /d "%~dp0"
echo ==========================================
echo   Starting FinchSpeak SFU Server...
echo   QUIC: udp://0.0.0.0:4433
echo   REST: http://0.0.0.0:8443
echo ==========================================
cargo run -p finch-server
pause
