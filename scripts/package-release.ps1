param([string]$OutputDirectory = "dist")
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$output = [System.IO.Path]::GetFullPath((Join-Path $root $OutputDirectory))
if (-not $output.StartsWith($root + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Release output must be inside the repository"
}
$serverExe = Join-Path $root "target/release/finch-server.exe"
$clientExe = Join-Path $root "target/release/finch-app.exe"
foreach ($file in @($serverExe, $clientExe)) {
    if (-not (Test-Path -LiteralPath $file)) { throw "Build release binaries first: $file" }
}
New-Item -ItemType Directory -Force -Path $output | Out-Null
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
if not exist finch-server.toml copy finch-server.toml.example finch-server.toml >nul
finch-server.exe
pause
'@ | Set-Content -LiteralPath (Join-Path $server "Run_Server.bat") -Encoding ascii
Copy-Item -Path (Join-Path $server "*") -Destination $portable
Copy-Item -LiteralPath $clientExe -Destination $portable
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
