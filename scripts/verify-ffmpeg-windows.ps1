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
if ($manifest.schemaVersion -ne $lock.schemaVersion) { throw "FFmpeg schema mismatch: expected $($lock.schemaVersion), got $($manifest.schemaVersion)" }
if ($manifest.sdkRevision -ne $lock.sdkRevision) { throw "FFmpeg SDK revision mismatch: expected $($lock.sdkRevision), got $($manifest.sdkRevision)" }
if ($manifest.license -ne $lock.license) { throw "FFmpeg license mismatch: expected $($lock.license), got $($manifest.license)" }
if ($manifest.linkMode -ne $lock.linkMode) { throw "FFmpeg link mode mismatch: expected $($lock.linkMode), got $($manifest.linkMode)" }
if ($manifest.platform -ne 'windows' -or $manifest.architecture -ne 'x86_64') { throw "Unexpected FFmpeg SDK platform or architecture: $($manifest.platform)/$($manifest.architecture)" }
$expectedPlatform = $lock.platforms.'windows-x86_64'
if ($manifest.source -ne $expectedPlatform.source.url) { throw "FFmpeg source mismatch: expected $($expectedPlatform.source.url), got $($manifest.source)" }
if ($manifest.archiveSha256 -ne $expectedPlatform.source.sha256) { throw "FFmpeg archive checksum mismatch: expected $($expectedPlatform.source.sha256), got $($manifest.archiveSha256)" }
$expectedLibraries = @($lock.requiredLibraries | Sort-Object)
$actualLibraries = @($manifest.requiredLibraries | Sort-Object)
if (($expectedLibraries -join ',') -ne ($actualLibraries -join ',')) { throw 'FFmpeg required library set mismatch' }
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
  # requiredLibraries is the complete SDK/runtime closure shipped beside the
  # executable. requiredRuntimeImports is the smaller set this MP4-only binary
  # must reference directly; transitive/link-only libraries such as avdevice
  # are still packaged and verified above without requiring a direct import.
  foreach ($name in $lock.requiredRuntimeImports) {
    if ($imports -notlike "*$name*.dll*") { throw "Executable import table does not contain FFmpeg library $name" }
  }
  if ($imports -like '*ffmpeg.exe*' -or $imports -like '*ffprobe.exe*' -or $imports -like '*ffplay.exe*') { throw 'FFmpeg CLI executable was linked into the application' }
}
Write-Host "Windows FFmpeg linkage verified: $binaryPath"
