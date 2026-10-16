param([string]$Archive = "dist/FinchSpeak-server-windows.zip", [string]$ExpectedVersion = "", [int]$HttpPort = 8443, [int]$QuicPort = 4433)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$archivePath = (Resolve-Path -LiteralPath (Join-Path $root $Archive)).Path
$version = if ($ExpectedVersion) { $ExpectedVersion } else { (Get-Content -LiteralPath (Join-Path $root 'app/tauri.conf.json') | ConvertFrom-Json).version }
# Both Cyrillic and spaces, with a caller working directory outside the package.
$work = Join-Path $root ("target/launcher QA " + [char]0x416 + " " + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
Expand-Archive -LiteralPath $archivePath -DestinationPath $work
if ($HttpPort -ne 8443 -or $QuicPort -ne 4433) {
    $template = Join-Path $work 'finch-server.toml.example'
    $text = [IO.File]::ReadAllText($template).Replace('http_port = 8443', "http_port = $HttpPort").Replace('quic_port = 4433', "quic_port = $QuicPort")
    [IO.File]::WriteAllText($template, $text)
    Write-Output "Test-only port overrides: HTTPS $HttpPort, QUIC $QuicPort"
}
$emptyInput = Join-Path $work 'input.txt'
[IO.File]::WriteAllText($emptyInput, '')
$launcher = $null
$serverIds = @()
function Start-Launcher([string]$Directory) {
    $script:launcher = Start-Process -FilePath $env:ComSpec -ArgumentList ('/d /c call "' + (Join-Path $Directory 'Run_Server.bat') + '"') -WorkingDirectory $root -WindowStyle Hidden -PassThru -RedirectStandardInput $emptyInput -RedirectStandardOutput (Join-Path $Directory 'stdout.log') -RedirectStandardError (Join-Path $Directory 'stderr.log')
}
function Remember-Server {
    $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($launcher.Id) AND Name='finch-server.exe'")
    $script:serverIds = @($children | Where-Object { $_.ExecutablePath -eq (Join-Path $work 'finch-server.exe') } | ForEach-Object { $_.ProcessId })
}
function Stop-Launcher {
    if ($launcher) {
        $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($launcher.Id) AND Name='finch-server.exe'")
        $script:serverIds += @($children | Where-Object { $_.ExecutablePath -eq (Join-Path $work 'finch-server.exe') } | ForEach-Object { $_.ProcessId })
    }
    foreach ($serverId in $script:serverIds) {
        $owned = Get-Process -Id $serverId -ErrorAction SilentlyContinue
        if ($owned -and $owned.Path -eq (Join-Path $work 'finch-server.exe')) { Stop-Process -Id $serverId; $owned.WaitForExit() }
    }
    $script:serverIds = @()
    if ($launcher -and -not $launcher.WaitForExit(3000)) { Stop-Process -Id $launcher.Id }
}
function Wait-Healthy {
    for ($i = 0; $i -lt 100; $i++) {
        Remember-Server
        if ($launcher.HasExited) { throw "Launcher failed: $(Get-Content (Join-Path $work 'stderr.log') -Raw)" }
        try {
            $health = Invoke-RestMethod "https://127.0.0.1:$HttpPort/health" -SkipCertificateCheck -TimeoutSec 1
            if ($serverIds.Count -eq 1 -and $health.version -eq $version) { return }
        } catch {}
        Start-Sleep -Milliseconds 100
    }
    throw 'Default-config launcher did not become healthy'
}
function Wait-Failure([string]$Directory, [string]$Pattern, [int]$ExpectedExit = -1) {
    if (-not $launcher.WaitForExit(15000)) { throw 'Failure did not exit promptly' }
    if ($launcher.ExitCode -eq 0 -or ($ExpectedExit -ge 0 -and $launcher.ExitCode -ne $ExpectedExit)) { throw "Wrong failure exit: $($launcher.ExitCode)" }
    $output = (Get-Content (Join-Path $Directory 'stdout.log') -Raw) + (Get-Content (Join-Path $Directory 'stderr.log') -Raw)
    if ($output -notmatch $Pattern) { throw "Missing diagnostic '$Pattern': $output" }
}
# Avoid mistaking another server's health for our own or disrupting an existing server.
$tcp = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Any, $HttpPort)
$udp = [Net.Sockets.UdpClient]::new()
try {
    $tcp.Server.ExclusiveAddressUse = $true
    $tcp.Start()
    $udp.Client.ExclusiveAddressUse = $true
    $udp.Client.Bind([Net.IPEndPoint]::new([Net.IPAddress]::Any, $QuicPort))
} finally { $tcp.Stop(); $udp.Dispose() }
try {
    Start-Launcher $work
    Wait-Healthy
    if ((Get-Content (Join-Path $work 'stdout.log') -Raw) -notmatch 'FinchSpeak .+ ready') { throw 'No visible readiness status' }
    if ((Get-Content (Join-Path $work 'stderr.log') -Raw) -match 'Error reading the log directory/files') { throw 'Fresh startup reports a false log directory error' }
    $config = Join-Path $work 'finch-server.toml'
    $configHash = (Get-FileHash -LiteralPath $config).Hash
    if ($configHash -ne (Get-FileHash -LiteralPath (Join-Path $work 'finch-server.toml.example')).Hash) { throw 'Launcher changed shipped defaults' }
    $cert = (Get-FileHash -LiteralPath (Join-Path $work 'data/tls/cert.der')).Hash
    Stop-Launcher
    Start-Launcher $work
    Wait-Healthy
    if ((Get-FileHash -LiteralPath $config).Hash -ne $configHash -or (Get-FileHash -LiteralPath (Join-Path $work 'data/tls/cert.der')).Hash -ne $cert) { throw 'Restart replaced config or certificate' }
    Stop-Launcher
    # The default HTTP port is already occupied: show the actual bind error and exit.
    $tcp.Start()
    try {
        Start-Launcher $work
        Wait-Failure $work 'Failed to bind HTTP listener'
        $logs = Get-ChildItem -LiteralPath (Join-Path $work 'data/logs') -File
        if (-not ($logs | Select-String -Pattern 'Failed to bind HTTP listener')) { throw 'Startup failure was not saved in server logs' }
    } finally { Remember-Server; Stop-Launcher; $tcp.Stop() }
    $busyUdp = [Net.Sockets.UdpClient]::new()
    try {
        $busyUdp.Client.ExclusiveAddressUse = $true
        $busyUdp.Client.Bind([Net.IPEndPoint]::new([Net.IPAddress]::Any, $QuicPort))
        Start-Launcher $work
        Wait-Failure $work 'Failed to bind QUIC endpoint'
    } finally { Remember-Server; Stop-Launcher; $busyUdp.Dispose() }
    [IO.File]::WriteAllText($config, '[broken')
    Start-Launcher $work
    Wait-Failure $work 'Failed to load server configuration'
    if ([IO.File]::ReadAllText($config) -ne '[broken') { throw 'Invalid operator configuration was overwritten' }
    Copy-Item -LiteralPath (Join-Path $work 'finch-server.toml.example') -Destination $config -Force
    $keyPath = Join-Path $work 'data/tls/key.der'
    [IO.File]::WriteAllText($keyPath, 'invalid test key')
    Start-Launcher $work
    Wait-Failure $work 'Failed to load TLS certificate'
    if ([IO.File]::ReadAllText($keyPath) -ne 'invalid test key') { throw 'Invalid TLS key was silently replaced' }
    $partial = Join-Path $work 'partial ZIP extraction'
    New-Item -ItemType Directory -Path $partial | Out-Null
    Copy-Item -LiteralPath (Join-Path $work 'Run_Server.bat') -Destination $partial
    Start-Launcher $partial
    Wait-Failure $partial 'Extract ALL files' 2
    Write-Output 'PASS: actual BAT, shipped defaults, foreign cwd, Cyrillic/spaces, restart, stable config/TLS, TCP/UDP conflicts, invalid config/TLS, missing executable'
    Write-Output "Launcher evidence: $work"
} finally { Stop-Launcher }
