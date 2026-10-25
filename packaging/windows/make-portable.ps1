<#
.SYNOPSIS
  Make a portable PhotoDesk folder and zip from a release build. Optionally bundle DarkRaw and
  LightRaw so the three programs travel together as one suite.

.DESCRIPTION
  Build first:  cargo build --release
  Then:         powershell -ExecutionPolicy Bypass -File packaging\windows\make-portable.ps1

  To bundle the other programs, pass the portable folders they already have:
    -DarkRaw  C:\path\to\DarkRaw\dist\DarkRaw-portable
    -LightRaw C:\path\to\LightRaw.it\dist\LightRaw-portable
  They are copied into DarkRaw\ and LightRaw\ inside the PhotoDesk folder, where PhotoDesk looks.
#>
param(
  [string]$Bin = (Join-Path $PSScriptRoot '..\..\target\release'),
  [string]$Out = (Join-Path $PSScriptRoot '..\..\dist\PhotoDesk-portable'),
  [string]$DarkRaw = '',
  [string]$LightRaw = ''
)
$ErrorActionPreference = 'Stop'
$Root = Resolve-Path (Join-Path $PSScriptRoot '..\..')

$Exe = Join-Path $Bin 'photodesk.exe'
if (-not (Test-Path $Exe)) { throw "photodesk.exe not found in $Bin. Run: cargo build --release" }

if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = Resolve-Path $Out

Copy-Item $Exe (Join-Path $Out 'PhotoDesk.exe')
Copy-Item (Join-Path $PSScriptRoot 'portable.txt') $Out
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Out }
}
if ($DarkRaw -ne '') {
  if (-not (Test-Path $DarkRaw)) { throw "DarkRaw folder not found: $DarkRaw" }
  Copy-Item -Recurse $DarkRaw (Join-Path $Out 'DarkRaw')
}
if ($LightRaw -ne '') {
  if (-not (Test-Path $LightRaw)) { throw "LightRaw folder not found: $LightRaw" }
  Copy-Item -Recurse $LightRaw (Join-Path $Out 'LightRaw')
}

$objdump = Get-Command objdump -ErrorAction SilentlyContinue
if ($objdump) {
  $dlls = & $objdump.Source -p $Exe | Select-String 'DLL Name:' | ForEach-Object { ($_ -split ':', 2)[1].Trim() }
  $extra = $dlls | Where-Object { $_ -match '^(libgcc|libstdc\+\+|libwinpthread|libgomp)' }
  if ($extra) {
    Write-Warning "PhotoDesk.exe needs DLLs that aren't part of Windows: $($extra -join ', '). Copy them from your WinLibs mingw64\bin folder into $Out."
  } else {
    Write-Host 'No extra DLLs needed.'
  }
}

$Zip = "$Out.zip"
if (Test-Path $Zip) { Remove-Item -Force $Zip }
Compress-Archive -Path $Out -DestinationPath $Zip
Write-Host "Portable folder: $Out"
Write-Host "Portable zip:    $Zip"
