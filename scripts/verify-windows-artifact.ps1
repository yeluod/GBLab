param(
  [string]$BundleRoot = 'target\release\bundle',
  [string]$LockFile = 'toolchains/ffmpeg-sdk.lock.json'
)

$ErrorActionPreference = 'Stop'

function Require-File([string]$Path, [string]$Description) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "$Description not found: $Path"
  }
}

$nsis = Get-ChildItem -Path (Join-Path $BundleRoot 'nsis') -Filter '*-setup.exe' -File | Select-Object -First 1
$msi = Get-ChildItem -Path (Join-Path $BundleRoot 'msi') -Filter '*.msi' -File | Select-Object -First 1
if ($null -eq $nsis) { throw 'No Windows NSIS installer generated.' }
if ($null -eq $msi) { throw 'No Windows MSI installer generated.' }
Require-File $LockFile 'FFmpeg lockfile'

$extractRoot = Join-Path $env:RUNNER_TEMP 'gblab-msi-contents'
if (Test-Path $extractRoot) { Remove-Item -Recurse -Force $extractRoot }
New-Item -ItemType Directory -Force -Path $extractRoot | Out-Null
$process = Start-Process msiexec.exe -Wait -PassThru -ArgumentList @('/a', "`"$($msi.FullName)`"", '/qn', "TARGETDIR=`"$extractRoot`"")
if ($process.ExitCode -ne 0) { throw "MSI administrative extraction failed with exit code $($process.ExitCode)." }

$bundleFiles = Get-ChildItem $extractRoot -File -Recurse
$application = $bundleFiles | Where-Object { $_.Name -eq 'gblab-desktop.exe' } | Select-Object -First 1
if ($null -eq $application) { throw 'Extracted MSI is missing gblab-desktop.exe.' }
$lock = Get-Content -Raw $LockFile | ConvertFrom-Json
foreach ($name in $lock.requiredLibraries) {
  if ($null -eq ($bundleFiles | Where-Object { $_.Name -like "*$name*.dll" } | Select-Object -First 1)) {
    throw "Extracted MSI is missing FFmpeg DLL: $name"
  }
}
if ($null -eq ($bundleFiles | Where-Object { $_.Name -eq 'manifest.json' } | Select-Object -First 1)) { throw 'Extracted MSI is missing FFmpeg manifest.' }
if ($null -eq ($bundleFiles | Where-Object { $_.Name -eq 'FFMPEG-LICENSE.txt' } | Select-Object -First 1)) { throw 'Extracted MSI is missing FFmpeg license.' }
if ($bundleFiles.Name -match '^(ffmpeg|ffprobe|ffplay)\.exe$') { throw 'Extracted MSI must not contain FFmpeg CLI executables.' }

$dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
if ($null -ne $dumpbin) {
  $headers = & $dumpbin.Source /HEADERS $application.FullName | Out-String
  if ($headers -notmatch 'machine \(x64\)') { throw 'Extracted application executable is not x64.' }
} else {
  $bytes = [System.IO.File]::ReadAllBytes($application.FullName)
  if ($bytes.Length -lt 64) { throw 'Extracted application executable is too small to be a PE file.' }
  $peOffset = [System.BitConverter]::ToInt32($bytes, 0x3c)
  if ($peOffset -lt 0 -or $peOffset + 6 -gt $bytes.Length) { throw 'Extracted application executable has an invalid PE header.' }
  if ($bytes[$peOffset] -ne 0x50 -or $bytes[$peOffset + 1] -ne 0x45 -or $bytes[$peOffset + 2] -ne 0x00 -or $bytes[$peOffset + 3] -ne 0x00) { throw 'Extracted application executable is not a PE file.' }
  $machine = [System.BitConverter]::ToUInt16($bytes, $peOffset + 4)
  if ($machine -ne 0x8664) { throw 'Extracted application executable is not x64.' }
}

Write-Host "Windows artifacts verified: NSIS=$($nsis.Name) MSI=$($msi.Name)"
