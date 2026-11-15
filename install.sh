#!/usr/bin/env bash
set -euo pipefail

REPO="phaze7r/copyzer"
VERSION="${COPYZER_VERSION:-v0.3.0}"
OS="$(uname -s)"
ARCH="$(uname -m)"

# On Arch Linux / Omarchy (unless --user or --tarball is requested), configure the [copyzer] pacman repo
if [[ "$OS" == "Linux" && -f /etc/arch-release && "${1:-}" != "--user" && "${1:-}" != "--tarball" ]]; then
  echo "Detected Arch Linux / Omarchy — configuring [copyzer] Pacman repository..."
  SUDO=""
  if [[ "$(id -u)" -ne 0 ]]; then
    SUDO="sudo"
  fi

  if ! grep -q '^\[copyzer\]' /etc/pacman.conf; then
    $SUDO tee -a /etc/pacman.conf >/dev/null <<EOF

[copyzer]
SigLevel = Optional TrustAll
Server = https://github.com/${REPO}/releases/latest/download
EOF
    echo "Added [copyzer] repository to /etc/pacman.conf"
  fi

  $SUDO pacman -Sy --noconfirm copyzer
  echo "Installed copyzer via pacman! Future updates will install automatically with 'sudo pacman -Syu' or 'yay -Syu'."
  exit 0
fi

case "$OS" in
  Linux) PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *)
    echo "Unsupported OS for install.sh: $OS"
    echo "On Windows, download copyzer-${VERSION}-windows-x86_64.zip from:"
    echo "  https://github.com/${REPO}/releases/tag/${VERSION}"
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  arm64|aarch64)
    if [[ "$PLATFORM" == "macos" ]]; then
      ARCH="aarch64"
    else
      echo "Unsupported prebuilt architecture for ${PLATFORM}: $ARCH"
      echo "Install via Cargo instead: cargo install --git https://github.com/${REPO} --tag ${VERSION}"
      exit 1
    fi
    ;;
  *)
    echo "Unsupported prebuilt architecture: $ARCH"
    echo "Install via Cargo instead: cargo install --git https://github.com/${REPO} --tag ${VERSION}"
    exit 1
    ;;
esac

ARCHIVE="copyzer-${VERSION}-${PLATFORM}-${ARCH}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading Copyzer ${VERSION} (${PLATFORM}-${ARCH})..."
curl -fsSL "${BASE_URL}/${ARCHIVE}" -o "${TMP_DIR}/${ARCHIVE}"
curl -fsSL "${BASE_URL}/${ARCHIVE}.sha256" -o "${TMP_DIR}/${ARCHIVE}.sha256"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$TMP_DIR" && sha256sum -c "${ARCHIVE}.sha256")
elif command -v shasum >/dev/null 2>&1; then
  (cd "$TMP_DIR" && shasum -a 256 -c "${ARCHIVE}.sha256")
fi
tar -xzf "${TMP_DIR}/${ARCHIVE}" -C "${TMP_DIR}"

STAGE="${TMP_DIR}/copyzer-${VERSION}-${PLATFORM}-${ARCH}"

if [[ "${1:-}" == "--user" ]] || [[ ! -w /usr/local/bin && "$(id -u)" -ne 0 ]]; then
  BIN_DIR="${HOME}/.local/bin"
  mkdir -p "${BIN_DIR}"
  install -m755 "${STAGE}/copyzer" "${BIN_DIR}/copyzer"
  if [[ "$PLATFORM" == "linux" && -f "${STAGE}/copyzer.desktop" ]]; then
    APP_DIR="${HOME}/.local/share/applications"
    ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"
    install -Dm644 "${STAGE}/copyzer.desktop" "${APP_DIR}/copyzer.desktop"
    install -Dm644 "${STAGE}/copyzer.svg" "${ICON_DIR}/copyzer.svg"
  fi
  echo "Installed copyzer to ${BIN_DIR}/copyzer"
else
  mkdir -p /usr/local/bin
  install -m755 "${STAGE}/copyzer" "/usr/local/bin/copyzer"
  if [[ "$PLATFORM" == "linux" && -f "${STAGE}/copyzer.desktop" ]]; then
    install -Dm644 "${STAGE}/copyzer.desktop" "/usr/share/applications/copyzer.desktop"
    install -Dm644 "${STAGE}/copyzer.svg" "/usr/share/icons/hicolor/scalable/apps/copyzer.svg"
    if [[ -f "${STAGE}/copyzer-usb-a.conf" ]]; then
      install -Dm644 "${STAGE}/copyzer-usb-a.conf" "/etc/modprobe.d/copyzer-usb-a.conf"
      modprobe -r apple_mfi_fastcharge 2>/dev/null || true
    fi
    if [[ -f "${STAGE}/90-copyzer-iphone.rules" ]]; then
      install -Dm644 "${STAGE}/90-copyzer-iphone.rules" "/etc/udev/rules.d/90-copyzer-iphone.rules"
      udevadm control --reload-rules 2>/dev/null || true
      udevadm trigger --subsystem-match=usb --action=add 2>/dev/null || true
    fi
  fi
  echo "Installed copyzer to /usr/local/bin/copyzer"
fi

if [[ "$PLATFORM" == "macos" ]]; then
  echo "Make sure libimobiledevice (and optionally macfuse + ifuse) is installed via Homebrew: brew install libimobiledevice"
else
  echo "Make sure ifuse, libimobiledevice, and usbmuxd are installed via your distro package manager."
fi
