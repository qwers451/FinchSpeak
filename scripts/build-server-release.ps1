$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$previousFlags = $env:RUSTFLAGS
$previousEncoded = $env:CARGO_ENCODED_RUSTFLAGS
Push-Location $root
try {
    # Explicit target keeps CRT flags off host proc-macros. A separate directory
    # prevents a later client build from replacing the validated server artifact.
    $env:RUSTFLAGS = $null
    $env:CARGO_ENCODED_RUSTFLAGS = "-C" + [char]31 + "target-feature=+crt-static"
    cargo build --release --locked -p finch-server --target x86_64-pc-windows-msvc --target-dir target/server-static
    if ($LASTEXITCODE -ne 0) { throw "Static server build failed ($LASTEXITCODE)" }
    & "$PSScriptRoot/test-server-runtime.ps1" -Executable "target/server-static/x86_64-pc-windows-msvc/release/finch-server.exe"
} finally {
    $env:RUSTFLAGS = $previousFlags
    $env:CARGO_ENCODED_RUSTFLAGS = $previousEncoded
    Pop-Location
}
