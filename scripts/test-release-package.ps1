param([string]$Archive = "dist/FinchSpeak-server-windows.zip")
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$work = Join-Path $root ("target/release-smoke-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $work | Out-Null
Expand-Archive -LiteralPath (Join-Path $root $Archive) -DestinationPath $work
foreach ($file in @("finch-server.exe", "finch-server.toml.example", "Run_Server.bat", "README.txt")) {
    if (-not (Test-Path -LiteralPath (Join-Path $work $file))) { throw "Package is missing $file" }
}
$tcp = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$tcp.Start()
$httpPort = $tcp.LocalEndpoint.Port
$tcp.Stop()
$udp = [System.Net.Sockets.UdpClient]::new(0)
$quicPort = $udp.Client.LocalEndPoint.Port
$udp.Dispose()
@"
[server]
host = "127.0.0.1"
http_port = $httpPort
quic_port = $quicPort
[auth]
registration = "invite"
[backup]
dir = "./data/backups"
"@ | Set-Content -LiteralPath (Join-Path $work "finch-server.toml") -Encoding utf8
$base = "https://127.0.0.1:$httpPort"
$process = $null
function Start-TestServer {
    $script:process = Start-Process -FilePath (Join-Path $work "finch-server.exe") -WorkingDirectory $work -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $work "stdout.log") -RedirectStandardError (Join-Path $work "stderr.log")
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        if ($script:process.HasExited) { throw "Packaged server exited: $(Get-Content (Join-Path $work 'stderr.log') -Raw)" }
        try {
            $health = Invoke-RestMethod "$base/health" -SkipCertificateCheck -TimeoutSec 1
            if ($health.version -ne "1.0.0") { throw "Unexpected release version $($health.version)" }
            return
        } catch { if ($attempt -eq 99) { throw }; Start-Sleep -Milliseconds 100 }
    }
}
function Stop-TestServer {
    if ($script:process -and -not $script:process.HasExited) {
        Stop-Process -Id $script:process.Id
        $script:process.WaitForExit()
    }
}
function Request([string]$Method, [string]$Path, $Body = $null, [string]$Token = "") {
    $requestParams = @{ Uri = "$base$Path"; Method = $Method; SkipCertificateCheck = $true; TimeoutSec = 10 }
    if ($Token) { $requestParams.Headers = @{ Authorization = "Bearer $Token" } }
    if ($null -ne $Body) { $requestParams.Body = $Body | ConvertTo-Json -Depth 8; $requestParams.ContentType = "application/json" }
    Invoke-RestMethod @requestParams
}
try {
    Start-TestServer
    $admin = Request POST /api/auth/register @{ username="testadmin"; password="test-release-password"; display_name="Test Admin" }
    if ($admin.user.role -ne "admin") { throw "First account did not become admin" }
    $invite = Request POST /api/invites @{ max_uses=1; expires_hours=1 } $admin.token
    $member = Request POST /api/auth/register @{ username="testmember"; password="test-release-password"; display_name="Test Member"; invite_code=$invite.code }
    if ($member.user.role -ne "member") { throw "Invited user is not a member" }
    $channels = Request GET /api/channels $null $member.token
    if (@($channels).Count -lt 2) { throw "Default channels missing" }
    Request PATCH /api/auth/me @{ display_name="Test Restored" } $member.token | Out-Null
    Request POST /api/admin/backup @{} $admin.token | Out-Null
    $backup = Get-ChildItem -LiteralPath (Join-Path $work "data/backups") -File | Sort-Object Name | Select-Object -Last 1
    if (-not $backup) { throw "Backup was not created" }
    $certPath = Join-Path $work "data/tls/cert.der"
    $fingerprint = (Get-FileHash -LiteralPath $certPath -Algorithm SHA256).Hash
    Stop-TestServer
    Start-TestServer
    $login = Request POST /api/auth/login @{ username="testmember"; password="test-release-password" }
    if ($login.user.display_name -ne "Test Restored") { throw "Profile did not survive restart" }
    if ((Get-FileHash -LiteralPath $certPath -Algorithm SHA256).Hash -ne $fingerprint) { throw "TLS certificate changed after restart" }
    Request PATCH /api/auth/me @{ display_name="After Backup" } $login.token | Out-Null
    Stop-TestServer
    # Only isolated test data are restored. Preserve the displaced database and WAL files.
    foreach ($name in @("finchspeak.db", "finchspeak.db-wal", "finchspeak.db-shm")) {
        $path = Join-Path $work $name
        if (Test-Path -LiteralPath $path) { Move-Item -LiteralPath $path -Destination "$path.before-restore" }
    }
    Copy-Item -LiteralPath $backup.FullName -Destination (Join-Path $work "finchspeak.db")
    Start-TestServer
    $restored = Request POST /api/auth/login @{ username="testmember"; password="test-release-password" }
    if ($restored.user.display_name -ne "Test Restored") { throw "Database backup was not restored" }
    Write-Output "PASS: packaged 1.0.0 server, health, admin bootstrap, invitation, member, channels, profile, restart, stable TLS certificate, backup and restore"
    Write-Output "Isolated test data: $work"
} finally { Stop-TestServer }
