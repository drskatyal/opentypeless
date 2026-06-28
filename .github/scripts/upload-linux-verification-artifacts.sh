#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${TAG_NAME:-}" ]]; then
  echo "::error::TAG_NAME is required to upload Linux verification artifacts."
  exit 1
fi

for name in LINUX_GPG_KEY_ID LINUX_GPG_PASSPHRASE; do
  if [[ -z "${!name:-}" ]]; then
    echo "::error::$name is required to sign Linux verification artifacts."
    exit 1
  fi
done

verification_dir="release-verification/linux-x86_64"
mkdir -p "$verification_dir"

mapfile -d '' artifacts < <(
  find src-tauri/target/release/bundle -type f \
    \( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \) \
    -print0 | sort -z
)

if (( ${#artifacts[@]} == 0 )); then
  echo "::error::No Linux release artifacts were found."
  exit 1
fi

sha_file="$verification_dir/SHA256SUMS-linux-x86_64.txt"
: > "$sha_file"

for artifact in "${artifacts[@]}"; do
  sha256sum "$artifact" | sed "s#  .*#  $(basename "$artifact")#" >> "$sha_file"

  signature_path="$verification_dir/$(basename "$artifact").asc"
  gpg --batch --yes --pinentry-mode loopback \
    --passphrase "$LINUX_GPG_PASSPHRASE" \
    --local-user "$LINUX_GPG_KEY_ID" \
    --armor --detach-sign \
    --output "$signature_path" \
    "$artifact"
done

gpg --batch --yes --pinentry-mode loopback \
  --passphrase "$LINUX_GPG_PASSPHRASE" \
  --local-user "$LINUX_GPG_KEY_ID" \
  --armor --detach-sign \
  --output "${sha_file}.asc" \
  "$sha_file"

gpg --armor --export "$LINUX_GPG_KEY_ID" > "$verification_dir/OpenTypeless-Linux-GPG-KEY.asc"

if compgen -G "src-tauri/target/release/bundle/appimage/*.AppImage" >/dev/null; then
  for appimage in src-tauri/target/release/bundle/appimage/*.AppImage; do
    chmod +x "$appimage"
    "$appimage" --appimage-signature >/dev/null
  done
fi

if compgen -G "src-tauri/target/release/bundle/rpm/*.rpm" >/dev/null; then
  sudo rpm --import "$verification_dir/OpenTypeless-Linux-GPG-KEY.asc"
  rpm --checksig -v src-tauri/target/release/bundle/rpm/*.rpm
fi

gh release upload "$TAG_NAME" "$verification_dir"/* \
  --repo tover0314-w/opentypeless \
  --clobber
