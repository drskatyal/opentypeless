$ErrorActionPreference = "Stop"

$hasCertificate = -not [string]::IsNullOrWhiteSpace($env:WINDOWS_CERTIFICATE)
$hasPassword = -not [string]::IsNullOrWhiteSpace($env:WINDOWS_CERTIFICATE_PASSWORD)
$allowUnsigned = $env:ALLOW_UNSIGNED_WINDOWS -eq 'true'

if (-not $hasCertificate -and -not $hasPassword) {
  if ($allowUnsigned) {
    Write-Warning "Unsigned Windows release explicitly allowed for this manual dispatch."
    exit 0
  }
  Write-Error "Windows signing certificate is required. Configure WINDOWS_CERTIFICATE and WINDOWS_CERTIFICATE_PASSWORD, or publish Windows through the Release Windows SignPath workflow."
}

if (-not $hasCertificate -or -not $hasPassword) {
  Write-Error "WINDOWS_CERTIFICATE and WINDOWS_CERTIFICATE_PASSWORD must be configured together."
}

$certificateDir = Join-Path $env:RUNNER_TEMP "opentypeless-windows-certificate"
New-Item -ItemType Directory -Force -Path $certificateDir | Out-Null

$pfxPath = Join-Path $certificateDir "certificate.pfx"
$encodedCertificate = $env:WINDOWS_CERTIFICATE `
  -replace "-----BEGIN [^-]+-----", "" `
  -replace "-----END [^-]+-----", "" `
  -replace "\s", ""
$certificateBytes = [Convert]::FromBase64String($encodedCertificate)
[IO.File]::WriteAllBytes($pfxPath, $certificateBytes)

$password = ConvertTo-SecureString -String $env:WINDOWS_CERTIFICATE_PASSWORD -Force -AsPlainText
$certificate = Import-PfxCertificate -FilePath $pfxPath -CertStoreLocation Cert:\CurrentUser\My -Password $password

if (-not $certificate -or [string]::IsNullOrWhiteSpace($certificate.Thumbprint)) {
  Write-Error "Windows certificate import failed or did not return a thumbprint."
}

$timestampUrl = $env:WINDOWS_TIMESTAMP_URL
if ([string]::IsNullOrWhiteSpace($timestampUrl)) {
  $timestampUrl = "http://timestamp.digicert.com"
}

$env:WINDOWS_CERTIFICATE_THUMBPRINT = $certificate.Thumbprint
$env:WINDOWS_TIMESTAMP_URL = $timestampUrl

node -e @"
const fs = require('fs');
const path = 'src-tauri/tauri.conf.json';
const config = JSON.parse(fs.readFileSync(path, 'utf8'));
config.bundle = config.bundle || {};
config.bundle.windows = {
  ...(config.bundle.windows || {}),
  certificateThumbprint: process.env.WINDOWS_CERTIFICATE_THUMBPRINT,
  digestAlgorithm: 'sha256',
  timestampUrl: process.env.WINDOWS_TIMESTAMP_URL,
};
fs.writeFileSync(path, JSON.stringify(config, null, 2) + '\n');
"@

Add-Content -Path $env:GITHUB_ENV -Value "WINDOWS_CERTIFICATE_THUMBPRINT=$($certificate.Thumbprint)"
Add-Content -Path $env:GITHUB_ENV -Value "WINDOWS_TIMESTAMP_URL=$timestampUrl"

Write-Host "Imported Windows code signing certificate with thumbprint $($certificate.Thumbprint)."
