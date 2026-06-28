#!/usr/bin/env bash
set -euo pipefail

for name in LINUX_GPG_PRIVATE_KEY LINUX_GPG_KEY_ID LINUX_GPG_PASSPHRASE; do
  if [[ -z "${!name:-}" ]]; then
    echo "::error::$name is required to sign Linux release artifacts."
    exit 1
  fi
done

export GNUPGHOME="${RUNNER_TEMP}/opentypeless-gnupg"
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"
echo "allow-loopback-pinentry" > "$GNUPGHOME/gpg-agent.conf"

key_path="${RUNNER_TEMP}/opentypeless-linux-release-key.asc"
if ! printf '%s' "$LINUX_GPG_PRIVATE_KEY" | base64 --decode > "$key_path" 2>/dev/null; then
  printf '%s' "$LINUX_GPG_PRIVATE_KEY" > "$key_path"
fi
chmod 600 "$key_path"

gpg --batch --yes --import "$key_path"
gpg --batch --list-secret-keys "$LINUX_GPG_KEY_ID" >/dev/null

{
  echo "GNUPGHOME=$GNUPGHOME"
  echo "SIGN=1"
  echo "SIGN_KEY=$LINUX_GPG_KEY_ID"
  echo "APPIMAGETOOL_FORCE_SIGN=1"
  echo "TAURI_SIGNING_RPM_KEY<<__OPENTYPELESS_RPM_KEY__"
  cat "$key_path"
  echo "__OPENTYPELESS_RPM_KEY__"
} >> "$GITHUB_ENV"

echo "Imported Linux GPG release key $LINUX_GPG_KEY_ID."
