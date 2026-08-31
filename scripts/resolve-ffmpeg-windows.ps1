param(
  [string]$OutputFile = '.ffmpeg-sdk\windows-source.json',
  [string]$LockFile = 'toolchains/ffmpeg-sdk.lock.json'
)

$ErrorActionPreference = 'Stop'
$lock = Get-Content -Raw -Path $LockFile | ConvertFrom-Json
$source = $lock.platforms.'windows-x86_64'.source
if ($source.kind -ne 'btbn-latest-release') { throw "Unsupported Windows FFmpeg source kind: $($source.kind)" }
if ([string]::IsNullOrWhiteSpace($source.releaseApiUrl)) { throw 'Windows FFmpeg release API URL is missing' }
if ([string]::IsNullOrWhiteSpace($source.assetName)) { throw 'Windows FFmpeg asset name is missing' }

$headers = @{
  Accept = 'application/vnd.github+json'
  'User-Agent' = 'GBLab-FFmpeg-SDK-Resolver'
  'X-GitHub-Api-Version' = '2022-11-28'
}
if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_TOKEN)) {
  $headers.Authorization = "Bearer $($env:GITHUB_TOKEN)"
}

try {
  $release = Invoke-RestMethod -MaximumRedirection 5 -Headers $headers -Uri $source.releaseApiUrl
} catch {
  throw "Unable to resolve the Windows FFmpeg release from $($source.releaseApiUrl). $($_.Exception.Message)"
}

$matches = @($release.assets | Where-Object { $_.name -eq $source.assetName -and $_.state -eq 'uploaded' })
if ($matches.Count -ne 1) {
  throw "Expected exactly one uploaded Windows FFmpeg asset named $($source.assetName), found $($matches.Count)"
}
$asset = $matches[0]
$digest = [string]$asset.digest
if ($digest -notmatch '^sha256:([0-9a-fA-F]{64})$') {
  throw "Windows FFmpeg asset does not expose a valid SHA-256 digest: $digest"
}
$archiveSha256 = $Matches[1].ToLowerInvariant()
$downloadUri = [Uri]$asset.browser_download_url
$expectedDownloadPrefix = '/BtbN/FFmpeg-Builds/releases/download/'
if ($downloadUri.Scheme -ne 'https' -or $downloadUri.Host -ne 'github.com' -or -not $downloadUri.AbsolutePath.StartsWith($expectedDownloadPrefix) -or -not $downloadUri.AbsolutePath.EndsWith("/$($source.assetName)")) {
  throw "Windows FFmpeg asset returned an unexpected download URL: $($asset.browser_download_url)"
}

$metadata = [ordered]@{
  sourceKind = $source.kind
  releaseId = [string]$release.id
  releaseTag = [string]$release.tag_name
  assetId = [string]$asset.id
  assetName = [string]$asset.name
  downloadUrl = [string]$asset.browser_download_url
  archiveSha256 = $archiveSha256
}
$parent = Split-Path -Parent $OutputFile
if (-not [string]::IsNullOrWhiteSpace($parent)) {
  New-Item -ItemType Directory -Force -Path $parent | Out-Null
}
$metadata | ConvertTo-Json | Set-Content -Path $OutputFile -Encoding UTF8

if ($env:GITHUB_OUTPUT) {
  "fingerprint=$archiveSha256" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
}
Write-Host "Resolved Windows FFmpeg SDK asset: name=$($asset.name) release=$($release.tag_name) sha256=$archiveSha256"
