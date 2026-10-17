param([string]$Installer='', [string]$OutputDirectory='target/install-qa')
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$version=(Get-Content "$root/app/tauri.conf.json" | ConvertFrom-Json).version
if(!$Installer){$target=if($env:CARGO_TARGET_DIR){$env:CARGO_TARGET_DIR}else{Join-Path $root 'target'};$Installer=Join-Path $target "release/bundle/nsis/FinchSpeak_${version}_x64-setup.exe"}
$Installer=(Resolve-Path -LiteralPath $Installer).Path
$uninstallKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\FinchSpeak'
if(Test-Path $uninstallKey){throw 'Existing user FinchSpeak installation: use a clean runner instead'}
if(Get-Process finch-app -ErrorAction SilentlyContinue){throw 'Existing FinchSpeak process: do not disturb it; use a clean runner'}
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$work=(Resolve-Path -LiteralPath $OutputDirectory).Path
$install=Join-Path $work ('install-'+[guid]::NewGuid().ToString('N'))
$profile=Join-Path $work 'isolated-profile'
New-Item -ItemType Directory -Force $profile | Out-Null
$marker=Join-Path $profile 'preserve-marker.txt'
'isolated upgrade data' | Set-Content $marker
function Install([string]$File){
 $p=Start-Process -FilePath $File -ArgumentList @('/S',"/D=$install") -WindowStyle Hidden -PassThru -Wait
 if($p.ExitCode -ne 0){throw "Installer failed $($p.ExitCode)"}
 if(!(Test-Path "$install/finch-app.exe")){throw 'Installed client missing'}
}
function Uninstall {
 $file=Join-Path $install 'uninstall.exe'
 if(Test-Path $file){$p=Start-Process -FilePath $file -ArgumentList @('/S',"_?=$install") -WindowStyle Hidden -PassThru -Wait;if($p.ExitCode){throw 'Uninstall failed'}}
}
function Smoke {
 & "$PSScriptRoot/test-client-runtime.ps1" -Executable "$install/finch-app.exe"
 $savedPath=$env:PATH;$savedProfile=$env:FINCHSPEAK_PROFILE_DIR;$savedWebview=$env:WEBVIEW2_USER_DATA_FOLDER
 $p=$null
 try {
  $env:PATH="$env:SystemRoot\System32";$env:FINCHSPEAK_PROFILE_DIR=$profile;$env:WEBVIEW2_USER_DATA_FOLDER=Join-Path $profile 'webview'
  $p=Start-Process "$install/finch-app.exe" -WorkingDirectory $work -WindowStyle Hidden -PassThru
  Start-Sleep -Seconds 6
  if($p.HasExited){throw "Installed client exited $($p.ExitCode)"}
 }finally{if($p -and !$p.HasExited){Stop-Process -Id $p.Id;$p.WaitForExit()};$env:PATH=$savedPath;$env:FINCHSPEAK_PROFILE_DIR=$savedProfile;$env:WEBVIEW2_USER_DATA_FOLDER=$savedWebview}
}
try {
 Install $Installer
 Smoke
 $freshHash=(Get-FileHash "$install/finch-app.exe").Hash
 $freshDllHash=(Get-FileHash "$install/finch_webrtc.dll").Hash
 foreach($name in @('native-audio-provenance.json','native-audio-dependencies.json','THIRD-PARTY-NOTICES.md','licenses/webrtc-webrtc-PATENTS','licenses/earshot-LICENSE-MIT','licenses/nnnoiseless-COPYING','licenses/opus-COPYING')){if(!(Test-Path (Join-Path $install $name))){throw "Installed resource missing: $name"}}
 if((Get-Item "$install/finch-app.exe").VersionInfo.ProductVersion -ne $version){throw 'Fresh install version mismatch'}
 Uninstall
 $old=Join-Path $work 'published-1.0.1'
 New-Item -ItemType Directory -Force $old | Out-Null
 gh release download v1.0.1 --repo qwers451/FinchSpeak --pattern 'FinchSpeak_1.0.1_x64-setup.exe' --pattern SHA256SUMS.txt --dir $old --clobber
 if($LASTEXITCODE){throw 'Published old installer download failed'}
 $oldInstaller=Join-Path $old 'FinchSpeak_1.0.1_x64-setup.exe'
 $expected=((Get-Content "$old/SHA256SUMS.txt" | Where-Object {$_ -match '  FinchSpeak_1.0.1_x64-setup.exe$'}) -split '  ')[0]
 if((Get-FileHash $oldInstaller).Hash -ne $expected){throw 'Published old installer checksum mismatch'}
 Install $oldInstaller
 if((Get-Item "$install/finch-app.exe").VersionInfo.ProductVersion -ne '1.0.1'){throw 'Old install version mismatch'}
 Install $Installer
 Smoke
 if((Get-FileHash "$install/finch-app.exe").Hash -ne $freshHash -or (Get-FileHash "$install/finch_webrtc.dll").Hash -ne $freshDllHash){throw 'Upgrade differs from fresh installation'}
 if((Get-Content $marker -Raw).Trim() -ne 'isolated upgrade data'){throw 'Upgrade changed isolated profile data'}
 Uninstall
 if(Test-Path "$install/finch-app.exe"){throw 'Uninstall left application binary'}
 if(!(Test-Path $marker)){throw 'Uninstall removed isolated profile data'}
 [pscustomobject]@{fresh_install='PASS';upgrade_1_0_1='PASS';native_runtime='PASS';installed_exe_sha256=$freshHash;installed_dll_sha256=$freshDllHash;isolated_marker_preserved='PASS';uninstall='PASS';auto_update_ui='NOT RUN: installer path tested, not the old client confirmation UI'} | ConvertTo-Json | Set-Content "$work/report.json" -Encoding utf8
 Write-Output 'PASS fresh installation,1.0.1 upgrade,installed runtime,resource hashes,data marker preservation,uninstall'
}finally{Uninstall}
