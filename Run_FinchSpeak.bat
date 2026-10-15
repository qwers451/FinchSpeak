@echo off
title FinchSpeak Desktop Client
cd /d "%~dp0"
echo ==========================================
echo   Launching FinchSpeak Desktop Client...
echo ==========================================
start "" "%~dp0target\release\finch-app.exe"
exit
