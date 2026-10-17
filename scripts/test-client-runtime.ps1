param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. "$PSScriptRoot/pe-imports.ps1"
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$native=Join-Path $directory 'finch_webrtc.dll'
if(!(Test-Path -LiteralPath $native)){throw 'Client native APM DLL missing'}
$pending=[Collections.Generic.Queue[string]]::new();$pending.Enqueue($Executable)
$seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
while($pending.Count){
    $binary=$pending.Dequeue()
    if(!$seen.Add($binary)){continue}
    $imports=@(Get-PeImports $binary)
    Write-Output "PE $(Split-Path -Leaf $binary): $($imports -join ', ')"
    foreach($name in $imports){
        $local=Join-Path $directory $name
        if(Test-Path -LiteralPath $local){$pending.Enqueue($local);continue}
        if($name -match '^(vcruntime|msvcp|msvcr|concrt)\d.*\.dll$'){throw "Unbundled Visual C++ runtime: $name"}
        if($name -match '^api-ms-win-.*\.dll$' -or $name -match '^ext-ms-win-.*\.dll$'){continue}
        if(!(Test-Path -LiteralPath (Join-Path "$env:SystemRoot/System32" $name))){throw "Unresolved import $name from $binary"}
    }
}
Write-Output "PASS x64 client/native DLL dependency closure: $($seen.Count) payload binaries; no developer PATH lookup"
