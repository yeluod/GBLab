param(
  [string]$OutputRoot = '.ffmpeg-sdk\\windows',
  [string]$LockFile = 'toolchains/ffmpeg-sdk.lock.json'
)

$ErrorActionPreference = 'Stop'
$architecture = 'x86_64'
$lock = Get-Content -Raw -Path $LockFile | ConvertFrom-Json
$platform = $lock.platforms.'windows-x86_64'
$source = $platform.source
$root = (New-Item -ItemType Directory -Force -Path $OutputRoot).FullName
$archive = Join-Path $root $source.filename
$extractRoot = Join-Path $root 'extract'

function Get-Sha256([string]$Path) {
  return (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

function Download-Archive {
  Write-Host "Downloading FFmpeg SDK: platform=windows architecture=$architecture version=$($lock.ffmpegVersion) url=$($source.url)"
  try {
    Invoke-WebRequest -MaximumRedirection 5 -Uri $source.url -OutFile $archive
  } catch {
    throw "FFmpeg SDK download failed: platform=windows architecture=$architecture filename=$($source.filename) url=$($source.url). $($_.Exception.Message)"
  }
}

if (Test-Path $archive) {
  $actual = Get-Sha256 $archive
  if ($actual -ne $source.sha256) {
    Write-Error "FFmpeg SDK checksum mismatch: expected=$($source.sha256) actual=$actual file=$archive"
    Remove-Item -Force $archive
    Download-Archive
  }
} else {
  Download-Archive
}
$actual = Get-Sha256 $archive
if ($actual -ne $source.sha256) { throw "FFmpeg SDK checksum mismatch after download: expected=$($source.sha256) actual=$actual" }

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
  source = $source.url
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
