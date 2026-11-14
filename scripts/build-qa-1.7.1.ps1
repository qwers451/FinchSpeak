param([Parameter(Mandatory = $true)][string]$NativeWebRtcRoot)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$qa = Join-Path $repo 'target/release-qa-v1.7.1'
$previousConfig = $env:TAURI_CONFIG
$previousNative = $env:FINCH_WEBRTC_ROOT
$previousJobs = $env:CARGO_BUILD_JOBS
Push-Location $repo
try {
    if (!(Test-Path -LiteralPath (Join-Path $NativeWebRtcRoot 'finch_webrtc.dll'))) { throw 'Pass the prepared native Release directory.' }
    $env:FINCH_WEBRTC_ROOT = (Resolve-Path -LiteralPath $NativeWebRtcRoot).Path
    $env:CARGO_BUILD_JOBS = '2'
    New-Item -ItemType Directory -Force -Path $qa | Out-Null
    foreach ($role in @('sender', 'viewer')) {
        # Startup registers every configured URI scheme. QA must not replace the installed
        # finchspeak:// handler; the empty scheme list leaves that registry entry untouched.
        $env:TAURI_CONFIG = @{
            identifier = "com.finchspeak.qa$role"
            plugins = @{ 'deep-link' = @{ desktop = @{ schemes = @() } } }
        } | ConvertTo-Json -Depth 8 -Compress
        cargo build --release --locked -p finch-app --bin finch-app *> "$qa/build-$role.log"
        if ($LASTEXITCODE -ne 0) { throw "QA $role build failed; see $qa/build-$role.log" }
        $destination = Join-Path $qa $role
        New-Item -ItemType Directory -Force -Path $destination | Out-Null
        Copy-Item -LiteralPath 'target/release/finch-app.exe' -Destination $destination -Force
        Copy-Item -LiteralPath (Join-Path $env:FINCH_WEBRTC_ROOT 'finch_webrtc.dll') -Destination $destination -Force
        $binaryText = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes((Join-Path $destination 'finch-app.exe')))
        if (!$binaryText.Contains("com.finchspeak.qa$role")) { throw "QA identifier missing from $role binary" }
        $binaryText = $null
        Write-Output "Built QA $role"
    }
    # Restore the normal application identifier in target/release; never install or launch it.
    $env:TAURI_CONFIG = $null
    cargo build --release --locked -p finch-app --bin finch-app *> "$qa/build-normal.log"
    if ($LASTEXITCODE -ne 0) { throw "Normal identifier rebuild failed; see $qa/build-normal.log" }
    cargo build --release --locked -p finch-server --bin finch-server *> "$qa/build-server.log"
    if ($LASTEXITCODE -ne 0) { throw "Server build failed; see $qa/build-server.log" }
    $serverBin = Join-Path $qa 'server/bin'
    New-Item -ItemType Directory -Force -Path $serverBin | Out-Null
    Copy-Item -LiteralPath 'target/release/finch-server.exe' -Destination $serverBin -Force
    $objects = Get-ChildItem -LiteralPath 'target/release/build' -Recurse -File -Include 'cpuid.o','coeff.o' |
        Where-Object { $_.FullName -match 'openh264-sys2' } | Group-Object DirectoryName |
        Where-Object { ($_.Group.Name -contains 'cpuid.o') -and ($_.Group.Name -contains 'coeff.o') }
    if (!$objects) { throw 'OpenH264 NASM objects were not found.' }
    Write-Output 'Built QA clients/server and restored the normal app identifier; OpenH264 NASM objects verified.'
} finally {
    $env:TAURI_CONFIG = $previousConfig
    $env:FINCH_WEBRTC_ROOT = $previousNative
    $env:CARGO_BUILD_JOBS = $previousJobs
    Pop-Location
}
