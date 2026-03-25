#!/bin/sh
# gitsift installer — downloads the latest release binary for your platform.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/MonteYin/gitsift/main/install.sh | bash
#
# Environment variables:
#   INSTALL_DIR  — where to install (default: /usr/local/bin)
#   VERSION      — specific version to install (default: latest)

set -eu

REPO="MonteYin/gitsift"
BIN_NAME="gitsift"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# --- Detect OS ---
detect_os() {
  case "$(uname -s)" in
    Linux*)  echo "unknown-linux-musl" ;;
    Darwin*) echo "apple-darwin" ;;
    *)       echo "unsupported" ;;
  esac
}

# --- Detect architecture ---
detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64)  echo "x86_64" ;;
    aarch64|arm64)  echo "aarch64" ;;
    *)              echo "unsupported" ;;
  esac
}

# --- Fetch latest version from GitHub API ---
get_latest_version() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
  else
    echo "Error: curl or wget is required" >&2
    exit 1
  fi
}

# --- Download file ---
download() {
  local url="$1" dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$dest"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$dest" "$url"
  fi
}

# --- Main ---
main() {
  local os arch target version archive_name url tmpdir

  os=$(detect_os)
  arch=$(detect_arch)

  if [ "$os" = "unsupported" ]; then
    echo "Error: unsupported OS: $(uname -s)" >&2
    exit 1
  fi
  if [ "$arch" = "unsupported" ]; then
    echo "Error: unsupported architecture: $(uname -m)" >&2
    exit 1
  fi

  # No x86_64 macOS build available
  if [ "$os" = "apple-darwin" ] && [ "$arch" = "x86_64" ]; then
    echo "Error: macOS x86_64 is not supported. Use macOS ARM (Apple Silicon) or install via 'cargo install --path .'" >&2
    exit 1
  fi

  target="${arch}-${os}"
  version="${VERSION:-$(get_latest_version)}"

  if [ -z "$version" ]; then
    echo "Error: could not determine latest version" >&2
    exit 1
  fi

  archive_name="${BIN_NAME}-${version}-${target}.tar.gz"
  url="https://github.com/${REPO}/releases/download/${version}/${archive_name}"

  echo "Installing ${BIN_NAME} ${version} (${target})..."

  TMPDIR_CLEANUP=$(mktemp -d)
  tmpdir="$TMPDIR_CLEANUP"
  trap 'rm -rf "$TMPDIR_CLEANUP"' EXIT

  echo "Downloading ${url}..."
  download "$url" "${tmpdir}/${archive_name}"

  # Verify SHA256 if checksum file is available
  local checksum_url="${url}.sha256"
  if download "$checksum_url" "${tmpdir}/${archive_name}.sha256" 2>/dev/null; then
    echo "Verifying checksum..."
    local expected actual
    expected=$(awk '{print $1}' "${tmpdir}/${archive_name}.sha256")
    if command -v sha256sum >/dev/null 2>&1; then
      actual=$(sha256sum "${tmpdir}/${archive_name}" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
      actual=$(shasum -a 256 "${tmpdir}/${archive_name}" | awk '{print $1}')
    else
      echo "Warning: no sha256sum or shasum found, skipping checksum verification" >&2
      actual="$expected"
    fi
    if [ "$expected" != "$actual" ]; then
      echo "Error: checksum mismatch (expected ${expected}, got ${actual})" >&2
      exit 1
    fi
    echo "Checksum verified."
  fi

  # Extract
  tar xzf "${tmpdir}/${archive_name}" -C "${tmpdir}"

  # Install
  local bin_path="${tmpdir}/${BIN_NAME}-${version}-${target}/${BIN_NAME}"
  if [ ! -f "$bin_path" ]; then
    echo "Error: binary not found in archive" >&2
    exit 1
  fi

  mkdir -p "$INSTALL_DIR" 2>/dev/null || true
  if [ -w "$INSTALL_DIR" ]; then
    cp "$bin_path" "${INSTALL_DIR}/${BIN_NAME}"
    chmod +x "${INSTALL_DIR}/${BIN_NAME}"
  else
    echo "Installing to ${INSTALL_DIR} (requires sudo)..."
    sudo mkdir -p "$INSTALL_DIR"
    sudo cp "$bin_path" "${INSTALL_DIR}/${BIN_NAME}"
    sudo chmod +x "${INSTALL_DIR}/${BIN_NAME}"
  fi

  echo "Installed ${BIN_NAME} to ${INSTALL_DIR}/${BIN_NAME}"
  "${INSTALL_DIR}/${BIN_NAME}" --version
}

main
