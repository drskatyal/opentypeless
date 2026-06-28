$ErrorActionPreference = "Stop"

foreach ($name in @("WINDOWS_CERTIFICATE", "WINDOWS_CERTIFICATE_PASSWORD")) {
  if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
    Write-Error "$name is required to sign Windows release artifacts."
  }
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
