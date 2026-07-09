# Release Signing Setup

OpenTypeless releases are built in `toverwu-qaq/opentypeless` and published to
`tover0314-w/opentypeless`.

## Required GitHub Secrets

Set these secrets on `toverwu-qaq/opentypeless`, because that repository runs
the GitHub Actions workflow.

macOS:

- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`

Tauri updater:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Cross-repository publishing:

- `RELEASE_TOKEN`

Linux:

- `LINUX_GPG_PRIVATE_KEY`: base64-encoded ASCII-armored private GPG key
- `LINUX_GPG_KEY_ID`: GPG key ID or fingerprint
- `LINUX_GPG_PASSPHRASE`: GPG key passphrase

Windows:

- `WINDOWS_CERTIFICATE`: optional base64-encoded PFX code signing certificate
- `WINDOWS_CERTIFICATE_PASSWORD`: required when `WINDOWS_CERTIFICATE` is set
- `WINDOWS_TIMESTAMP_URL`: optional timestamp server URL; defaults to DigiCert

The release workflow publishes Windows artifacts even when Windows signing
secrets are not configured. In that case, the Windows installers are unsigned.

Windows SignPath:

- `SIGNPATH_API_TOKEN`: token for a SignPath user that is a submitter for the
  selected signing policy
- `SIGNPATH_ORGANIZATION_ID`: SignPath organization ID
- `SIGNPATH_PROJECT_SLUG`: SignPath project slug
- `SIGNPATH_SIGNING_POLICY_SLUG`: SignPath signing policy slug

The SignPath project and GitHub trusted build system must point to
`toverwu-qaq/opentypeless`, because that repository runs the GitHub Actions
workflow and owns the GitHub artifact submitted to SignPath. Signed Windows
artifacts are still published to `tover0314-w/opentypeless`.

The Windows SignPath workflow uses the project's default artifact
configuration. This default artifact configuration must have a `<zip-file>`
root because GitHub's `actions/upload-artifact` action stores files as a ZIP
archive.

## Windows Certificate Notes

Use a real code signing certificate. SSL/TLS certificates do not sign Windows
desktop apps. EV certificates get Microsoft SmartScreen reputation immediately;
OV certificates can still show SmartScreen warnings until reputation builds.

If you receive a `.pfx`, encode it before saving it as a GitHub secret:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("certificate.pfx")) |
  Set-Content -NoNewline windows-certificate-base64.txt
```

Save the content of `windows-certificate-base64.txt` as `WINDOWS_CERTIFICATE`.

## Linux GPG Notes

Generate a release-only GPG key, export it, and base64 encode it:

```bash
gpg --full-gen-key
gpg --armor --export-secret-keys "OpenTypeless Release" > opentypeless-linux-private.asc
openssl base64 -A -in opentypeless-linux-private.asc -out opentypeless-linux-private.asc.base64
gpg --list-secret-keys --keyid-format LONG
```

Save `opentypeless-linux-private.asc.base64` as `LINUX_GPG_PRIVATE_KEY`, the
fingerprint/key ID as `LINUX_GPG_KEY_ID`, and the passphrase as
`LINUX_GPG_PASSPHRASE`.

The workflow embeds an AppImage signature, signs RPM bundles through Tauri,
creates detached `.asc` signatures for Linux artifacts, and uploads
`SHA256SUMS-linux-x86_64.txt`.
