param([Parameter(Mandatory)][string]$Archive,[string]$OutputDirectory='target/external-server-qa',[switch]$KeepRunning)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$archivePath=(Resolve-Path -LiteralPath $Archive).Path
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$work=(Resolve-Path -LiteralPath $OutputDirectory).Path
if (Test-Path (Join-Path $work 'data')) { throw 'Use a fresh isolated server directory' }
Expand-Archive -LiteralPath $archivePath -DestinationPath $work
$tcp=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0);$tcp.Start();$httpPort=$tcp.LocalEndpoint.Port;$tcp.Stop()
$udp=[Net.Sockets.UdpClient]::new(0);$quicPort=$udp.Client.LocalEndPoint.Port;$udp.Dispose()
@"
[server]
host = "127.0.0.1"
http_port = $httpPort
quic_port = $quicPort
[auth]
registration = "invite"
"@ | Set-Content (Join-Path $work 'finch-server.toml') -Encoding utf8
$process=Start-Process (Join-Path $work 'finch-server.exe') -WorkingDirectory $work -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $work 'server-stdout.log') -RedirectStandardError (Join-Path $work 'server-stderr.log')
$success=$false
try {
    $base="https://127.0.0.1:$httpPort"
    $ready=$false
    for($i=0;$i -lt 100;$i++) {
        if($process.HasExited){throw 'Isolated server exited'}
        try {$health=Invoke-RestMethod "$base/health" -SkipCertificateCheck -TimeoutSec 1;$ready=$true;break}catch{Start-Sleep -Milliseconds 100}
    }
    if(!$ready){throw 'Server not ready'}
    $hash=(Get-FileHash (Join-Path $work 'data/tls/cert.der') -Algorithm SHA256).Hash
    $fingerprint=([regex]::Matches($hash,'..') | ForEach-Object Value) -join ':'
    $target=if($env:CARGO_TARGET_DIR){$env:CARGO_TARGET_DIR}else{Join-Path $root 'target'}
    & (Join-Path $target 'debug/examples/external_voice.exe') $base "127.0.0.1:$quicPort" $fingerprint
    if($LASTEXITCODE){throw 'Pinned compatibility probe failed'}
    [pscustomobject]@{pid=$process.Id;executable=$process.Path;http=$base;quic="127.0.0.1:$quicPort";fingerprint=$fingerprint;version=$health.version;accounts=@('compatadmin','compatmember');password='password_123'} | ConvertTo-Json | Set-Content (Join-Path $work 'qa-endpoint.json') -Encoding utf8
    $success=$true
    Write-Output "PASS external published/new server $($health.version); isolated endpoint $base; report $work/qa-endpoint.json"
} finally {
    if ((!$KeepRunning -or !$success) -and !$process.HasExited) { Stop-Process -Id $process.Id; $process.WaitForExit() }
}
