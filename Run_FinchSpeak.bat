@echo off
title FinchSpeak Desktop Client
cd /d "%~dp0"
if not exist "%~dp0target\release\finch-app.exe" (
  echo The client is not built yet. Running Build_FinchSpeak.bat first...
  call "%~dp0Build_FinchSpeak.bat"
)
echo ==========================================
echo   Launching FinchSpeak Desktop Client...
echo ==========================================
start "" "%~dp0target\release\finch-app.exe"
exit
