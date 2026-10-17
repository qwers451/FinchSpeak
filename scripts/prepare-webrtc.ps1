param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release',
    [string]$CacheDirectory = '',
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$definition = Join-Path $repo 'native/webrtc'
$lockPath = Join-Path $definition 'dependencies.lock.json'
$lock = Get-Content -LiteralPath $lockPath -Raw | ConvertFrom-Json
if (!$CacheDirectory) { $CacheDirectory = Join-Path $repo 'target/native-cache' }
$cache = [IO.Path]::GetFullPath($CacheDirectory)
New-Item -ItemType Directory -Force -Path $cache | Out-Null
function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed ($LASTEXITCODE)" }
}
function Assert-Hash($Path, $Expected) {
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Expected) { throw "Native dependency SHA256 mismatch: $Path" }
}
foreach ($entry in $lock.downloads) {
    $file = Join-Path $cache $entry.file
    if (!(Test-Path -LiteralPath $file)) {
        if ($Offline) { throw "Offline native dependency missing: $($entry.file)" }
        $temporary = "$file.partial"
        Invoke-WebRequest -Uri $entry.url -OutFile $temporary
        Assert-Hash $temporary $entry.sha256
        Move-Item -LiteralPath $temporary -Destination $file
    }
    Assert-Hash $file $entry.sha256
}
# Tools are build-only and kept local to the cache; the application does not invoke them.
$python = if ($env:FINCH_NATIVE_PYTHON) { $env:FINCH_NATIVE_PYTHON } else { (Get-Command python -ErrorAction Stop).Source }
$tools = Join-Path $cache 'tools'
if (!(Test-Path -LiteralPath (Join-Path $tools 'mesonbuild/__main__.py'))) {
    Invoke-Checked $python @('-m','pip','install','--no-index','--no-deps','--target',$tools,(Join-Path $cache 'meson-1.9.1-py3-none-any.whl'),(Join-Path $cache 'ninja-1.13.0-py3-none-win_amd64.whl'))
}
$oldPath = $env:PATH
$oldInclude = $env:INCLUDE
$oldLib = $env:LIB
$oldPythonPath = $env:PYTHONPATH
$oldCC = $env:CC
$oldCXX = $env:CXX
try {
    # CI uses an x64 developer environment. MozillaBuild's extracted MSVC is also supported.
    if (!$env:INCLUDE -and (Test-Path -LiteralPath 'E:/mozbuild/vs/VC/Tools/MSVC')) {
        $msvc = Get-ChildItem 'E:/mozbuild/vs/VC/Tools/MSVC' -Directory | Sort-Object Name -Descending | Select-Object -First 1
        $sdk = Get-ChildItem 'E:/mozbuild/vs/Windows Kits/10/Include' -Directory | Sort-Object Name -Descending | Select-Object -First 1
        $sdkRoot = Split-Path (Split-Path $sdk.FullName -Parent) -Parent
        $env:INCLUDE = @((Join-Path $msvc.FullName 'include')) + (@('ucrt','shared','um','winrt')).ForEach({Join-Path $sdk.FullName $_}) -join ';'
        $env:LIB = @((Join-Path $msvc.FullName 'lib/x64'),(Join-Path $sdkRoot "Lib/$($sdk.Name)/ucrt/x64"),(Join-Path $sdkRoot "Lib/$($sdk.Name)/um/x64")) -join ';'
    }
    $compiler = if ($env:FINCH_NATIVE_COMPILER) { $env:FINCH_NATIVE_COMPILER } elseif (Get-Command cl.exe -ErrorAction SilentlyContinue) { (Get-Command cl.exe).Source } elseif (Test-Path -LiteralPath 'E:/mozbuild/vs/VC/Tools/MSVC') { (Get-ChildItem 'E:/mozbuild/vs/VC/Tools/MSVC/*/bin/Hostx64/x64/cl.exe' | Sort-Object FullName -Descending | Select-Object -First 1).FullName } else { throw 'An x64 MSVC developer environment is required to build WebRTC' }
    $env:PATH = (Split-Path -Parent $compiler) + ';' + (Join-Path $tools 'bin') + ';' + $oldPath
    $env:CC = $compiler
    $env:CXX = $compiler
    $env:PYTHONPATH = $tools
    $source = Join-Path $cache 'source'
    $sourceStamp = Join-Path $source 'upstream.sha256'
    $upstreamHash = $lock.downloads[0].sha256
    if (!(Test-Path -LiteralPath $sourceStamp)) {
        $extract = Join-Path $cache 'extract'
        New-Item -ItemType Directory -Force -Path $extract, "$source/subprojects" | Out-Null
        Invoke-Checked 'tar' @('-xf',(Join-Path $cache 'webrtc-sys.crate'),'-C',$extract)
        Copy-Item -LiteralPath (Join-Path $extract 'webrtc-audio-processing-sys-2.1.0/webrtc-audio-processing') -Destination "$source/subprojects/apm" -Recurse
        New-Item -ItemType Directory -Force -Path "$source/subprojects/packagecache" | Out-Null
        foreach ($name in @('abseil-cpp-20240722.0.tar.gz','abseil-cpp_20240722.0-3_patch.zip')) { Copy-Item -LiteralPath (Join-Path $cache $name) -Destination "$source/subprojects/packagecache/$name" }
        Set-Content -LiteralPath $sourceStamp -Value $upstreamHash -Encoding ascii
    }
    if ((Get-Content -LiteralPath $sourceStamp -Raw).Trim() -ne $upstreamHash) { throw 'Native source cache belongs to a different lock; use a fresh cache directory' }
    Copy-Item -LiteralPath (Join-Path $definition 'meson.build') -Destination "$source/meson.build"
    New-Item -ItemType Directory -Force -Path "$source/probe" | Out-Null
    Copy-Item -LiteralPath (Join-Path $definition 'probe/main.cc') -Destination "$source/probe/main.cc"
    foreach ($name in @('bridge.cc','bridge.h')) { if (Test-Path -LiteralPath (Join-Path $definition $name)) { Copy-Item -LiteralPath (Join-Path $definition $name) -Destination "$source/$name" } }
    $identityText = (Get-FileHash -LiteralPath $compiler).Hash + (Get-FileHash -LiteralPath $lockPath).Hash + $env:INCLUDE + $env:LIB + 'c++20-MT'
    $fingerprint = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identityText))).Substring(0,12).ToLowerInvariant()
    $build = Join-Path $cache "$Configuration-$fingerprint"
    $buildType = if ($Configuration -eq 'Debug') { 'debug' } else { 'release' }
    $setup = @('-m','mesonbuild.mesonmain','setup',$build,$source,"--buildtype=$buildType",'--wrap-mode=nodownload','-Dcpp_std=c++20','-Dapm:cpp_std=c++20','-Dabseil-cpp:cpp_std=c++20')
    if (Test-Path -LiteralPath "$build/build.ninja") { $setup += '--reconfigure' }
    Invoke-Checked $python $setup
    Invoke-Checked (Join-Path $tools 'bin/ninja.exe') @('-C',$build,'-j','6')
    Invoke-Checked (Join-Path $build 'finch-webrtc-probe.exe') @()
    $stage = Join-Path $repo "target/native/$Configuration"
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    foreach ($name in @('finch_webrtc.dll','finch_webrtc.lib','finch_webrtc.dll.lib','finch-webrtc-probe.exe')) { if (Test-Path -LiteralPath "$build/$name") { Copy-Item -LiteralPath "$build/$name" -Destination "$stage/$name" } }
    $provenance = [ordered]@{ lock_sha256=(Get-FileHash $lockPath).Hash; configuration=$Configuration; build_fingerprint=$fingerprint; compiler=(& $compiler 2>&1 | Out-String).Trim(); include=$env:INCLUDE; crt='MT native DLL'; python=(& $python --version); probe='PASS'; upstream_revision=$lock.upstreamRevision; wrapper_revision=$lock.wrapperRevision }
    $provenance | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$stage/provenance.json" -Encoding utf8
    $env:FINCH_WEBRTC_ROOT = $stage
    $global:LASTEXITCODE = 0
    Write-Output "FINCH_WEBRTC_ROOT=$stage"
} finally {
    $env:PATH=$oldPath; $env:INCLUDE=$oldInclude; $env:LIB=$oldLib; $env:PYTHONPATH=$oldPythonPath; $env:CC=$oldCC; $env:CXX=$oldCXX
}
