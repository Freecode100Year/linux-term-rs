#!/usr/bin/env bash
set -euo pipefail

REPO="Freecode100Year/linux-term-rs"
BIN_NAME="linux-term-rs"

ARCH_RAW=$(uname -m)
case "${ARCH_RAW}" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "❌ Unsupported architecture: ${ARCH_RAW}"; exit 1 ;;
esac

echo "🚀 Detected architecture: ${ARCH}. Fetching latest static release from ${REPO}..."
RELEASE_URL=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep "browser_download_url.*${ARCH}-unknown-linux-musl.tar.gz\"" \
  | cut -d : -f 2,3 \
  | tr -d ' "')

if [ -z "$RELEASE_URL" ]; then
  echo "❌ Error: Release package for ${ARCH} not found."
  exit 1
fi

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

curl -sSL "$RELEASE_URL" | tar -xz -C "$TMP_DIR"
INSTALL_SRC=$(find "$TMP_DIR" -type f -name "$BIN_NAME")
chmod +x "$INSTALL_SRC"

DEST_DIR="/usr/local/bin"
if [ ! -w "$DEST_DIR" ]; then
  SUDO="sudo"
else
  SUDO=""
fi

$SUDO mv "$INSTALL_SRC" "${DEST_DIR}/${BIN_NAME}"
echo "✅ Installed successfully to ${DEST_DIR}/${BIN_NAME}. Run '${BIN_NAME}' to launch."
