param([string]$Executable = "target/server-static/x86_64-pc-windows-msvc/release/finch-server.exe")
$ErrorActionPreference = "Stop"
# Read the actual PE import directory: a developer PC having the runtime installed
# must not make a non-portable server pass release validation.
$bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Executable))
function U16([int]$Offset) { [BitConverter]::ToUInt16($bytes, $Offset) }
function U32([int]$Offset) { [BitConverter]::ToUInt32($bytes, $Offset) }
if ($bytes.Length -lt 64 -or (U16 0) -ne 0x5a4d) { throw "Not a Windows executable" }
$pe = [int](U32 60)
if ((U32 $pe) -ne 0x4550 -or (U16 ($pe + 4)) -ne 0x8664) { throw "Expected x64 PE executable" }
$sections = U16 ($pe + 6)
$optional = $pe + 24
if ((U16 $optional) -ne 0x20b) { throw "Expected PE32+ executable" }
$sectionStart = $optional + (U16 ($pe + 20))
function RvaOffset([uint32]$Rva) {
    for ($i = 0; $i -lt $sections; $i++) {
        $offset = $sectionStart + 40 * $i
        $va = U32 ($offset + 12)
        $size = [Math]::Max((U32 ($offset + 8)), (U32 ($offset + 16)))
        if ($Rva -ge $va -and $Rva -lt $va + $size) {
            return [int]($Rva - $va + (U32 ($offset + 20)))
        }
    }
    throw "PE RVA is not mapped: $Rva"
}
function ImportName([uint32]$Rva) {
    $start = RvaOffset $Rva
    $end = $start
    while ($end -lt $bytes.Length -and $bytes[$end] -ne 0) { $end++ }
    if ($end -eq $bytes.Length) { throw "Unterminated PE import name" }
    [Text.Encoding]::ASCII.GetString($bytes, $start, $end - $start)
}
$imports = @()
$importRva = U32 ($optional + 120)
if ($importRva) {
    $descriptor = RvaOffset $importRva
    while ((U32 ($descriptor + 12)) -ne 0) {
        $imports += ImportName (U32 ($descriptor + 12))
        $descriptor += 20
    }
}
# PE32+ delay-load descriptors use RVAs when Attributes bit 0 is set.
$delayRva = U32 ($optional + 112 + 13 * 8)
if ($delayRva) {
    $descriptor = RvaOffset $delayRva
    while ((U32 ($descriptor + 4)) -ne 0) {
        if (((U32 $descriptor) -band 1) -eq 0) { throw "Unsupported delay import addressing" }
        $imports += ImportName (U32 ($descriptor + 4))
        $descriptor += 32
    }
}
$runtime = @($imports | Where-Object { $_ -match '^(vcruntime|msvcp|msvcr|concrt)\d.*\.dll$' })
if ($runtime.Count) { throw "Server requires an unbundled Visual C++ runtime: $($runtime -join ', ')" }
Write-Output "PASS: x64 server has no external Visual C++ runtime imports ($($imports -join ', '))"
