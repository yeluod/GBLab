param(
  [string]$OutputRoot = '.ffmpeg-sdk\\windows',
  [string]$LockFile = 'toolchains/ffmpeg-sdk.lock.json',
  [string]$SourceMetadataFile = '.ffmpeg-sdk\\windows-source.json'
)

$ErrorActionPreference = 'Stop'
$architecture = 'x86_64'
$lock = Get-Content -Raw -Path $LockFile | ConvertFrom-Json
$platform = $lock.platforms.'windows-x86_64'
$source = $platform.source
$resolvedSource = Get-Content -Raw -Path $SourceMetadataFile | ConvertFrom-Json
if ($resolvedSource.sourceKind -ne $source.kind) { throw "Windows FFmpeg source kind mismatch: expected=$($source.kind) actual=$($resolvedSource.sourceKind)" }
if ($resolvedSource.assetName -ne $source.assetName) { throw "Windows FFmpeg asset mismatch: expected=$($source.assetName) actual=$($resolvedSource.assetName)" }
if ($resolvedSource.archiveSha256 -notmatch '^[0-9a-f]{64}$') { throw "Invalid resolved Windows FFmpeg SHA-256: $($resolvedSource.archiveSha256)" }
$root = (New-Item -ItemType Directory -Force -Path $OutputRoot).FullName
$archive = Join-Path $root $resolvedSource.assetName
$extractRoot = Join-Path $root 'extract'

function Get-Sha256([string]$Path) {
  return (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

function Download-Archive {
  Write-Host "Downloading FFmpeg SDK: platform=windows architecture=$architecture version=$($lock.ffmpegVersion) url=$($resolvedSource.downloadUrl)"
  try {
    Invoke-WebRequest -MaximumRedirection 5 -Uri $resolvedSource.downloadUrl -OutFile $archive
  } catch {
    throw "FFmpeg SDK download failed: platform=windows architecture=$architecture filename=$($resolvedSource.assetName) url=$($resolvedSource.downloadUrl). $($_.Exception.Message)"
  }
}

if (Test-Path $archive) {
  $actual = Get-Sha256 $archive
  if ($actual -ne $resolvedSource.archiveSha256) {
    Remove-Item -Force $archive
    Download-Archive
  }
} else {
  Download-Archive
}
$actual = Get-Sha256 $archive
if ($actual -ne $resolvedSource.archiveSha256) { throw "FFmpeg SDK checksum mismatch after download: expected=$($resolvedSource.archiveSha256) actual=$actual" }

if (Test-Path $extractRoot) { Remove-Item -Recurse -Force $extractRoot }
Expand-Archive -Path $archive -DestinationPath $extractRoot
$packageRoot = Get-ChildItem -Path $extractRoot -Directory -Recurse |
  Where-Object { (Test-Path (Join-Path $_.FullName 'include')) -and (Test-Path (Join-Path $_.FullName 'bin')) -and (Test-Path (Join-Path $_.FullName 'lib')) } |
  Select-Object -First 1
if ($null -eq $packageRoot) { throw "Unable to locate FFmpeg SDK layout in $extractRoot" }

foreach ($directory in @('include', 'lib', 'bin')) {
  $destination = Join-Path $root $directory
  if (Test-Path $destination) { Remove-Item -Recurse -Force $destination }
  Copy-Item -Recurse -Force (Join-Path $packageRoot.FullName $directory) $destination
}

foreach ($name in $lock.requiredLibraries) {
  if ($null -eq (Get-ChildItem (Join-Path $root 'lib') -Filter "*$name*.lib" | Select-Object -First 1)) { throw "Missing FFmpeg import library: $name" }
  if ($null -eq (Get-ChildItem (Join-Path $root 'bin') -Filter "*$name*.dll" | Select-Object -First 1)) { throw "Missing FFmpeg runtime DLL: $name" }
}

Copy-Item -Force $LockFile (Join-Path $root 'lockfile.json')
$manifest = [ordered]@{
  schemaVersion = $lock.schemaVersion
  sdkRevision = $lock.sdkRevision
  ffmpegVersion = $lock.ffmpegVersion
  platform = 'windows'
  architecture = $architecture
  linkMode = $lock.linkMode
  license = $lock.license
  requiredLibraries = @($lock.requiredLibraries)
  sourceKind = $resolvedSource.sourceKind
  sourceReleaseId = $resolvedSource.releaseId
  sourceReleaseTag = $resolvedSource.releaseTag
  sourceAssetId = $resolvedSource.assetId
  sourceAsset = $resolvedSource.assetName
  source = $resolvedSource.downloadUrl
  archiveSha256 = $actual
}
$manifest | ConvertTo-Json | Set-Content -Path (Join-Path $root 'manifest.json') -Encoding UTF8
$licenseFile = Get-ChildItem -Path $packageRoot.FullName -File -Recurse | Where-Object { $_.Name -match '^(LICENSE|COPYING).*' } | Select-Object -First 1
if ($null -eq $licenseFile) { throw 'FFmpeg license file was not found in SDK archive' }
Copy-Item -Force $licenseFile.FullName (Join-Path $root 'FFMPEG-LICENSE.txt')

if ($env:GITHUB_ENV) {
  @(
    "FFMPEG_INCLUDE_DIR=$(Join-Path $root 'include')"
    "FFMPEG_LIBS_DIR=$(Join-Path $root 'lib')"
    'FFMPEG_LINK_MODE=dynamic'
  ) | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
}
Write-Host "FFmpeg Windows SDK ready: $root"
