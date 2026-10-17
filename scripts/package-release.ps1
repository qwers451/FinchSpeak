param([string]$OutputDirectory = "dist", [string]$ClientTargetDirectory = "", [string]$ServerTargetDirectory = "target/server-static")
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = [System.IO.Path]::GetFullPath((Join-Path $root $OutputDirectory))
if (-not $output.StartsWith($root + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Release output must be inside the repository"
}
$serverExe = Join-Path $ServerTargetDirectory "x86_64-pc-windows-msvc/release/finch-server.exe"
if (!$ClientTargetDirectory) { $ClientTargetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' } }
$clientExe = Join-Path $ClientTargetDirectory "release/finch-app.exe"
$nativeDll = Join-Path $ClientTargetDirectory "release/finch_webrtc.dll"
$version = (Get-Content (Join-Path $root 'app/tauri.conf.json') | ConvertFrom-Json).version
if ((Get-Item -LiteralPath $clientExe).VersionInfo.ProductVersion -ne $version) { throw 'Client artifact version does not match release metadata' }
if ((Get-FileHash $nativeDll).Hash -ne (Get-FileHash (Join-Path $root 'target/native/Release/finch_webrtc.dll')).Hash) { throw 'Client native DLL differs from verified Release stage' }
& "$PSScriptRoot/test-client-runtime.ps1" -Executable $clientExe
foreach ($file in @($serverExe, $clientExe)) {
    if (-not (Test-Path -LiteralPath $file)) { throw "Build release binaries first: $file" }
}
New-Item -ItemType Directory -Force -Path $output | Out-Null
& "$PSScriptRoot/test-server-runtime.ps1" -Executable $serverExe
# Build in a fresh staging directory; never remove an existing server's data directory.
$stage = Join-Path $output ("staging-" + [guid]::NewGuid().ToString("N"))
$server = Join-Path $stage "FinchSpeak-server"
$portable = Join-Path $stage "FinchSpeak"
New-Item -ItemType Directory -Force -Path $server, $portable | Out-Null
Copy-Item -LiteralPath $serverExe -Destination $server
Copy-Item -LiteralPath (Join-Path $root "deploy/finch-server.windows.toml") -Destination (Join-Path $server "finch-server.toml.example")
Copy-Item -LiteralPath (Join-Path $root "deploy/README-PACKAGE.txt") -Destination (Join-Path $server "README.txt")
@'
@echo off
title FinchSpeak Server
cd /d "%~dp0"
if not exist "%~dp0finch-server.exe" (
  echo ERROR: finch-server.exe is missing. Extract ALL files from the ZIP first.
  echo Keep Run_Server.bat and finch-server.exe in the same writable folder.
  pause
  exit /b 2
)
if not exist finch-server.toml copy finch-server.toml.example finch-server.toml >nul
"%~dp0finch-server.exe"
set "server_exit=%errorlevel%"
if not "%server_exit%"=="0" echo Server stopped with error %server_exit%. Please copy the error text above.
pause
exit /b %server_exit%
'@ | Set-Content -LiteralPath (Join-Path $server "Run_Server.bat") -Encoding ascii
Copy-Item -Path (Join-Path $server "*") -Destination $portable
Copy-Item -LiteralPath $clientExe, $nativeDll -Destination $portable
Copy-Item -LiteralPath (Join-Path $root 'target/native/Release/provenance.json') -Destination (Join-Path $portable 'native-audio-provenance.json')
Copy-Item -LiteralPath (Join-Path $root 'native/webrtc/dependencies.lock.json') -Destination (Join-Path $portable 'native-audio-dependencies.json')
foreach ($destination in @($server, $portable)) {
    Copy-Item -LiteralPath (Join-Path $root 'docs/THIRD-PARTY-NOTICES.md') -Destination $destination
    Copy-Item -LiteralPath (Join-Path $root 'docs/licenses') -Destination $destination -Recurse
}
@'
@echo off
cd /d "%~dp0"
start "" "%~dp0finch-app.exe"
'@ | Set-Content -LiteralPath (Join-Path $portable "Run_FinchSpeak.bat") -Encoding ascii
Compress-Archive -Path (Join-Path $server "*") -DestinationPath (Join-Path $output "FinchSpeak-server-windows.zip") -Force
Compress-Archive -Path (Join-Path $portable "*") -DestinationPath (Join-Path $output "FinchSpeak-windows.zip") -Force
$artifacts = @("FinchSpeak-server-windows.zip", "FinchSpeak-windows.zip")
$checksums = foreach ($name in $artifacts) {
    $hash = (Get-FileHash -LiteralPath (Join-Path $output $name) -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $name"
}
$checksums | Set-Content -LiteralPath (Join-Path $output "SHA256SUMS.txt") -Encoding ascii
Write-Output "Release archives: $output"
