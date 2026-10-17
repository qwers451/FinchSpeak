$ErrorActionPreference='Stop'
& "$PSScriptRoot/prepare-tests.ps1"
$repo=Split-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) -Parent
$savedPath=$env:PATH
try {
  $env:PATH="$env:SystemRoot\System32"
  foreach ($configuration in @('Debug','Release')) {
    & "$repo/target/native/$configuration/finch-webrtc-probe.exe"
    if ($LASTEXITCODE -ne 0) { throw "$configuration probe failed without build tools" }
    $provenance=Get-Content "$repo/target/native/$configuration/provenance.json" -Raw | ConvertFrom-Json
    if ($provenance.probe -ne 'PASS' -or $provenance.configuration -ne $configuration) { throw 'Native provenance/configuration mismatch' }
  }
} finally { $env:PATH=$savedPath }
Write-Output 'PASS: Debug and Release native probes run with system PATH only'
