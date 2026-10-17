param([string]$OutputDirectory = '')
$ErrorActionPreference='Stop'
$repo=Split-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) -Parent
if (!$OutputDirectory) { $OutputDirectory=Join-Path $repo ('target/native-script-tests-'+[guid]::NewGuid().ToString('N')) }
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$missing=Join-Path $OutputDirectory 'missing'
try { & "$repo/scripts/prepare-webrtc.ps1" -Offline -CacheDirectory $missing; throw 'Missing dependency was accepted' }
catch { if ($_.Exception.Message -notlike '*Offline native dependency missing: webrtc-sys.crate*') { throw } }
$modified=Join-Path $OutputDirectory 'modified'
New-Item -ItemType Directory -Force -Path $modified | Out-Null
Set-Content -LiteralPath (Join-Path $modified 'webrtc-sys.crate') -Value 'modified dependency' -Encoding ascii
try { & "$repo/scripts/prepare-webrtc.ps1" -Offline -CacheDirectory $modified; throw 'Modified dependency was accepted' }
catch { if ($_.Exception.Message -notlike '*Native dependency SHA256 mismatch:*') { throw } }
Write-Output 'PASS: offline missing dependency and tampered dependency are rejected before build'
