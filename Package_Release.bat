@echo off
rem Builds the server and the desktop client in release mode and packs them into
rem dist\FinchSpeak-windows.zip: unzip anywhere, run Run_Server.bat / Run_FinchSpeak.bat.
title Package FinchSpeak
cd /d "%~dp0"

echo [1/4] Frontend...
cd app\frontend
call npm install
call npm run build
if errorlevel 1 goto :fail
cd ..\..

echo [2/4] Server and client (release)...
cargo build --release -p finch-server
if errorlevel 1 goto :fail
cargo build --release -p finch-app --features custom-protocol
if errorlevel 1 goto :fail

echo [3/4] Assembling dist\FinchSpeak...
if exist dist\FinchSpeak rmdir /s /q dist\FinchSpeak
mkdir dist\FinchSpeak
copy /y target\release\finch-server.exe dist\FinchSpeak\ >nul
copy /y target\release\finch-app.exe dist\FinchSpeak\ >nul
copy /y deploy\finch-server.windows.toml dist\FinchSpeak\finch-server.toml.example >nul
copy /y deploy\README-PACKAGE.txt dist\FinchSpeak\README.txt >nul
(
echo @echo off
echo title FinchSpeak Server
echo cd /d "%%~dp0"
echo if not exist finch-server.toml copy finch-server.toml.example finch-server.toml ^>nul
echo finch-server.exe
echo pause
) > dist\FinchSpeak\Run_Server.bat
(
echo @echo off
echo cd /d "%%~dp0"
echo start "" "%%~dp0finch-app.exe"
) > dist\FinchSpeak\Run_FinchSpeak.bat

echo [4/4] Zipping...
powershell -NoProfile -Command "Compress-Archive -Path dist\FinchSpeak\* -DestinationPath dist\FinchSpeak-windows.zip -Force"
if errorlevel 1 goto :fail
echo.
echo Done: dist\FinchSpeak-windows.zip
pause
exit /b 0
:fail
echo Packaging failed.
pause
exit /b 1
