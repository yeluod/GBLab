param(
  [string]$Binary = 'target\release\gblab-desktop.exe',
  [string]$SdkRoot = '.ffmpeg-sdk\windows'
)

$ErrorActionPreference = 'Stop'

$binaryPath = (Resolve-Path $Binary).Path
$binaryDirectory = Split-Path $binaryPath -Parent
$requiredLibraries = @('avcodec', 'avdevice', 'avfilter', 'avformat', 'avutil', 'swresample', 'swscale')
foreach ($name in $requiredLibraries) {
  if ($null -eq (Get-ChildItem -Path $binaryDirectory -Filter "*$name*.dll" | Select-Object -First 1)) {
    throw "FFmpeg DLL $name is not beside $binaryPath"
  }
}
$licensePath = Join-Path (Resolve-Path $SdkRoot) 'FFMPEG-LICENSE.txt'
if (-not (Test-Path $licensePath)) { throw "FFmpeg license file not found: $licensePath" }
$manifestPath = Join-Path (Resolve-Path $SdkRoot) 'manifest.json'
if (-not (Test-Path $manifestPath)) { throw "FFmpeg manifest not found: $manifestPath" }

$dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
if ($null -ne $dumpbin) {
  $imports = & $dumpbin.Source /DEPENDENTS $binaryPath | Out-String
  foreach ($name in $requiredLibraries) {
    if ($imports -notmatch "(?i)$name.*\.dll") { throw "Executable import table does not contain FFmpeg library $name" }
  }
}
Write-Host "Windows FFmpeg linkage verified: $binaryPath"
