param([ValidateRange(0.01,120)][double]$Minutes = 30, [Parameter(Mandatory)][string]$OutputDirectory, [string]$Executable = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (!$Executable) {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $Executable = Join-Path $target 'debug/examples/voice_soak.exe'
}
$Executable = (Resolve-Path -LiteralPath $Executable).Path
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$seconds = [int][Math]::Ceiling($Minutes * 60)
$report = Join-Path $OutputDirectory 'capture.json'
$process = Start-Process -FilePath $Executable -ArgumentList @($seconds, ('"' + $report + '"')) -WorkingDirectory $root -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $OutputDirectory 'stdout.log') -RedirectStandardError (Join-Path $OutputDirectory 'stderr.log')
$samples = [Collections.Generic.List[object]]::new()
$clock = [Diagnostics.Stopwatch]::StartNew()
try {
    while (!$process.HasExited) {
        $counter = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$($process.Id)" | Select-Object -First 1
        if ($counter) { $samples.Add([pscustomobject]@{ seconds=$clock.Elapsed.TotalSeconds; private_working_set=[long]$counter.WorkingSetPrivate; private_bytes=[long]$counter.PrivateBytes }) }
        Start-Sleep -Seconds 2
        $process.Refresh()
        if ($clock.Elapsed.TotalSeconds -gt $seconds + 60) { throw 'Soak process exceeded timeout' }
    }
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { throw "Capture failed: $(Get-Content (Join-Path $OutputDirectory 'stderr.log') -Raw)" }
    $capture = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    if ($capture.frames -ne $seconds * 50 -or $capture.processing_errors -ne 0 -or $capture.private_frames -eq 0) { throw 'Incomplete capture evidence' }
    $samples | ConvertTo-Json | Set-Content (Join-Path $OutputDirectory 'memory.json') -Encoding utf8
    if ($Minutes -ge 30) {
        $baseline = $samples | Where-Object seconds -ge 300 | Select-Object -First 1
        $final = $samples | Where-Object seconds -ge 1800 | Select-Object -First 1
        if (!$baseline -or !$final -or !$baseline.private_working_set -or !$final.private_working_set) { throw 'Private-working-set measurements missing' }
        $growth = $final.private_working_set - $baseline.private_working_set
        if ($growth -gt 20MB) { throw "Private-working-set growth exceeds20MiB: $growth" }
        [pscustomobject]@{baseline=$baseline;final=$final;growth_bytes=$growth} | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $OutputDirectory 'memory-verdict.json') -Encoding utf8
    }
    Write-Output "PASS: capture soak $seconds seconds; $($capture.frames) frames, $($capture.private_frames) private frames, $($capture.rejoins) recreations; p95 DSP $($capture.p95_dsp_us_upper)us, max $($capture.max_dsp_us)us."
} finally {
    if (!$process.HasExited) { Stop-Process -Id $process.Id; $process.WaitForExit() }
}
