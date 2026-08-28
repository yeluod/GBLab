param(
  [string]$OutputRoot = '.ffmpeg-sdk\windows'
)

$ErrorActionPreference = 'Stop'

$releaseId = '377995260'
$assetId = '532622813'
$assetSha256 = '54b56d8f7e3fdeb3a987650a93cf4d4ed2f446f893f109dce191deec2007d155'
$assetUrl = "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/assets/$assetId"
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
$root = (Resolve-Path $OutputRoot).Path
$archive = Join-Path $root 'ffmpeg-win64-lgpl-shared-8.1.zip'
$extractRoot = Join-Path $root 'extract'

if (-not (Test-Path $archive)) {
  Invoke-WebRequest -Headers @{ Accept = 'application/octet-stream' } -Uri $assetUrl -OutFile $archive
}

$actualSha256 = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLowerInvariant()
if ($actualSha256 -ne $assetSha256) {
  throw "FFmpeg archive checksum mismatch. Expected $assetSha256, got $actualSha256"
}

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

$requiredLibraries = @('avcodec', 'avdevice', 'avfilter', 'avformat', 'avutil', 'swresample', 'swscale')
foreach ($name in $requiredLibraries) {
  if (-not (Get-ChildItem (Join-Path $root 'lib') -Filter "*$name*.lib")) { throw "Missing FFmpeg import library: $name" }
  if (-not (Get-ChildItem (Join-Path $root 'bin') -Filter "*$name*.dll")) { throw "Missing FFmpeg runtime DLL: $name" }
}

$manifest = @{
  source = $assetUrl
  releaseId = $releaseId
  assetId = $assetId
  version = '8.1'
  archiveSha256 = $assetSha256
  platform = 'windows'
  architecture = 'x86_64'
  license = 'LGPL-2.1-or-later'
} | ConvertTo-Json
Set-Content -Path (Join-Path $root 'manifest.json') -Value $manifest -Encoding UTF8
$licenseFile = Get-ChildItem -Path $packageRoot.FullName -File -Recurse |
  Where-Object { $_.Name -match '^(LICENSE|COPYING).*' } |
  Select-Object -First 1
if ($null -eq $licenseFile) { throw "FFmpeg license file was not found in SDK archive" }
Copy-Item -Force $licenseFile.FullName (Join-Path $root 'FFMPEG-LICENSE.txt')

if ($env:GITHUB_ENV) {
  @(
    "FFMPEG_INCLUDE_DIR=$(Join-Path $root 'include')"
    "FFMPEG_LIBS_DIR=$(Join-Path $root 'lib')"
    'FFMPEG_LINK_MODE=dynamic'
  ) | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
}

Write-Host "FFmpeg Windows SDK ready: $root"
