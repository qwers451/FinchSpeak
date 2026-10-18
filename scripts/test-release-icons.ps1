param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$Installer,
    [string]$SourceIcon = "app/icons/icon.ico"
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$sourcePath = (Resolve-Path -LiteralPath $SourceIcon).Path
$referenceIcon = [System.Drawing.Icon]::new($sourcePath)
try {
    $source = $referenceIcon.ToBitmap()
    try {
        foreach ($path in @($Executable, $Installer)) {
            $resolved = (Resolve-Path -LiteralPath $path).Path
            $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($resolved)
            if (-not $icon) { throw "No embedded icon in $resolved" }
            try {
                $actual = $icon.ToBitmap()
                try {
                    if ($actual.Width -ne $source.Width -or $actual.Height -ne $source.Height) {
                        throw "Icon dimensions differ from the supplied bird: $resolved"
                    }
                    for ($y = 0; $y -lt $source.Height; $y++) {
                        for ($x = 0; $x -lt $source.Width; $x++) {
                            if ($actual.GetPixel($x, $y).ToArgb() -ne $source.GetPixel($x, $y).ToArgb()) {
                                throw "Embedded icon differs from the supplied bird: $resolved"
                            }
                        }
                    }
                } finally { $actual.Dispose() }
            } finally { $icon.Dispose() }
        }
    } finally { $source.Dispose() }
} finally { $referenceIcon.Dispose() }

Write-Output 'PASS client and installer embed the supplied bird icon'
