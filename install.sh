#!/usr/bin/env sh
set -eu

REPO="tamadeu/sun"
INSTALL_DIR="${SUN_INSTALL_DIR:-$HOME/.local/bin}"
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS/$ARCH" in
  Linux/x86_64) ASSET="sun-linux-x86_64.tar.gz" ;;
  Darwin/arm64|Darwin/aarch64) ASSET="sun-macos-aarch64.tar.gz" ;;
  Darwin/x86_64) ASSET="sun-macos-x86_64.tar.gz" ;;
  *)
    echo "Unsupported platform: $OS/$ARCH" >&2
    echo "Supported: Linux x86_64, macOS Apple Silicon and Intel." >&2
    echo "Windows users: see install.ps1 in the repository." >&2
    exit 1
    ;;
esac

URL="https://github.com/$REPO/releases/latest/download/$ASSET"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required to install Sun." >&2
  exit 1
fi

echo "Downloading Sun ($ASSET)..."
if ! curl --fail --location --silent --show-error "$URL" -o "$TMP_DIR/sun.tar.gz"; then
  echo "Could not download a Sun release. The project may not have published its first release yet." >&2
  echo "Check https://github.com/$REPO/releases" >&2
  exit 1
fi

tar -xzf "$TMP_DIR/sun.tar.gz" -C "$TMP_DIR"
if [ ! -f "$TMP_DIR/sun" ]; then
  echo "The downloaded archive does not contain the expected 'sun' executable." >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
install -m 755 "$TMP_DIR/sun" "$INSTALL_DIR/sun"
echo "Sun installed to $INSTALL_DIR/sun"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) echo "Run: sun --help" ;;
  *) echo "Add this directory to your PATH: $INSTALL_DIR"; echo "Then run: sun --help" ;;
esac
