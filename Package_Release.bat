@echo off
rem Builds the server and the desktop client in release mode and packs them into
rem dist\FinchSpeak-windows.zip: unzip anywhere, run Run_Server.bat / Run_FinchSpeak.bat.
title Package FinchSpeak
cd /d "%~dp0"

echo [1/4] Frontend...
cd app\frontend
call npm ci
call npm run build
if errorlevel 1 goto :fail
cd ..\..

echo [2/4] Server and client (release)...
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-server-release.ps1
if errorlevel 1 goto :fail
cargo build --release -p finch-app --features custom-protocol
if errorlevel 1 goto :fail

echo [3/4] Assembling and zipping release packages...
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\package-release.ps1
if errorlevel 1 goto :fail
echo.
echo Done: dist\FinchSpeak-windows.zip
pause
exit /b 0
:fail
echo Packaging failed.
pause
exit /b 1
