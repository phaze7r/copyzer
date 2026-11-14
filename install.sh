#!/usr/bin/env bash
set -euo pipefail

REPO="phaze7r/copyzer"
VERSION="${COPYZER_VERSION:-v0.1}"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  *)
    echo "Unsupported prebuilt architecture: $ARCH"
    echo "Install via Cargo instead: cargo install --git https://github.com/${REPO} --tag ${VERSION}"
    exit 1
    ;;
esac

ARCHIVE="copyzer-${VERSION}-linux-${ARCH}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading Copyzer ${VERSION} (${ARCH})..."
curl -fsSL "${BASE_URL}/${ARCHIVE}" -o "${TMP_DIR}/${ARCHIVE}"
curl -fsSL "${BASE_URL}/${ARCHIVE}.sha256" -o "${TMP_DIR}/${ARCHIVE}.sha256"

(cd "$TMP_DIR" && sha256sum -c "${ARCHIVE}.sha256")
tar -xzf "${TMP_DIR}/${ARCHIVE}" -C "${TMP_DIR}"

STAGE="${TMP_DIR}/copyzer-${VERSION}-linux-${ARCH}"

if [[ "${1:-}" == "--user" ]] || [[ ! -w /usr/local/bin && "$(id -u)" -ne 0 ]]; then
  BIN_DIR="${HOME}/.local/bin"
  APP_DIR="${HOME}/.local/share/applications"
  ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"
  install -Dm755 "${STAGE}/copyzer" "${BIN_DIR}/copyzer"
  install -Dm644 "${STAGE}/copyzer.desktop" "${APP_DIR}/copyzer.desktop"
  install -Dm644 "${STAGE}/copyzer.svg" "${ICON_DIR}/copyzer.svg"
  echo "Installed copyzer to ${BIN_DIR}/copyzer"
else
  install -Dm755 "${STAGE}/copyzer" "/usr/local/bin/copyzer"
  install -Dm644 "${STAGE}/copyzer.desktop" "/usr/share/applications/copyzer.desktop"
  install -Dm644 "${STAGE}/copyzer.svg" "/usr/share/icons/hicolor/scalable/apps/copyzer.svg"
  echo "Installed copyzer to /usr/local/bin/copyzer"
fi

echo "Make sure ifuse, libimobiledevice, and usbmuxd are installed via your distro package manager."
