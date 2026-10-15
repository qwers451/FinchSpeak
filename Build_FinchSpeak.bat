@echo off
title Build FinchSpeak Desktop Client
cd /d "%~dp0"
echo Building the frontend...
cd app\frontend
call npm install
call npm run build
if errorlevel 1 goto :fail
cd ..\..
echo Building the desktop client (release)...
cargo build --release -p finch-app --features custom-protocol
if errorlevel 1 goto :fail
echo.
echo Done: target\release\finch-app.exe  (start it with Run_FinchSpeak.bat)
pause
exit /b 0
:fail
echo Build failed.
pause
exit /b 1
