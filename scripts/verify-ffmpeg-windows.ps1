param(
  [string]$Binary = 'target\release\gblab-desktop.exe',
  [string]$SdkRoot = '.ffmpeg-sdk\windows',
  [string]$LockFile = 'toolchains/ffmpeg-sdk.lock.json',
  [switch]$SdkOnly
)

$ErrorActionPreference = 'Stop'
$sdkPath = (Resolve-Path $SdkRoot).Path
$lock = Get-Content -Raw -Path $LockFile | ConvertFrom-Json
$manifest = Get-Content -Raw -Path (Join-Path $sdkPath 'manifest.json') | ConvertFrom-Json
if ($manifest.ffmpegVersion -ne $lock.ffmpegVersion) { throw "FFmpeg version mismatch: expected $($lock.ffmpegVersion), got $($manifest.ffmpegVersion)" }
if ($manifest.license -ne $lock.license) { throw "FFmpeg license mismatch: expected $($lock.license), got $($manifest.license)" }
if ($manifest.platform -ne 'windows' -or $manifest.architecture -ne 'x86_64') { throw "Unexpected FFmpeg SDK platform or architecture: $($manifest.platform)/$($manifest.architecture)" }
if (-not (Test-Path (Join-Path $sdkPath 'FFMPEG-LICENSE.txt'))) { throw "FFmpeg license file not found in $sdkPath" }
if ($SdkOnly) {
  foreach ($name in $lock.requiredLibraries) {
    if ($null -eq (Get-ChildItem -Path (Join-Path $sdkPath 'lib') -Filter "*$name*.lib" | Select-Object -First 1)) { throw "FFmpeg import library missing from SDK: $name" }
    if ($null -eq (Get-ChildItem -Path (Join-Path $sdkPath 'bin') -Filter "*$name*.dll" | Select-Object -First 1)) { throw "FFmpeg runtime DLL missing from SDK: $name" }
  }
  Write-Host "Windows FFmpeg SDK verified: $sdkPath"
  exit 0
}
$binaryPath = (Resolve-Path $Binary).Path
$binaryDirectory = Split-Path $binaryPath -Parent
foreach ($name in $lock.requiredLibraries) {
  if ($null -eq (Get-ChildItem -Path $binaryDirectory -Filter "*$name*.dll" | Select-Object -First 1)) { throw "FFmpeg DLL $name is not beside $binaryPath" }
}
$dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
if ($null -ne $dumpbin) {
  $imports = & $dumpbin.Source /DEPENDENTS $binaryPath | Out-String
  foreach ($name in $lock.requiredLibraries) {
    if ($imports -notlike "*$name*.dll*") { throw "Executable import table does not contain FFmpeg library $name" }
  }
  if ($imports -like '*ffmpeg.exe*' -or $imports -like '*ffprobe.exe*' -or $imports -like '*ffplay.exe*') { throw 'FFmpeg CLI executable was linked into the application' }
}
Write-Host "Windows FFmpeg linkage verified: $binaryPath"
