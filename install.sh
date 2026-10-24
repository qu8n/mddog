#!/bin/sh
# Installs the latest mdd release and starts its warm spare at login.
#   curl -fsSL https://raw.githubusercontent.com/qu8n/mddog/main/install.sh | sh
# MDDOG_INSTALL_DIR picks the folder (default ~/.local/bin). MDDOG_NO_AGENT=1 skips the login agent.
set -eu

url=${MDDOG_RELEASE_URL:-https://github.com/qu8n/mddog/releases/latest/download}
dir=${MDDOG_INSTALL_DIR:-$HOME/.local/bin}

fail() { echo "mddog install: $*" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || fail "mddog runs only on macOS"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"
curl -fsSL -o mdd-macos.tar.gz "$url/mdd-macos.tar.gz" || fail "could not download $url/mdd-macos.tar.gz"
curl -fsSL -o mdd-macos.tar.gz.sha256 "$url/mdd-macos.tar.gz.sha256" || fail "could not download the checksum"
shasum -a 256 -c mdd-macos.tar.gz.sha256 >/dev/null || fail "checksum mismatch"
tar xzf mdd-macos.tar.gz

mkdir -p "$dir"
mv -f mdd "$dir/mdd"
xattr -d com.apple.quarantine "$dir/mdd" 2>/dev/null || true
echo "mddog install: $("$dir/mdd" --version) is at $dir/mdd"

# The agent records the binary's path, so it runs after the move.
[ "${MDDOG_NO_AGENT:-}" = 1 ] || "$dir/mdd" --install-agent

case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "mddog install: $dir is not on your PATH. Add it, for example: echo 'export PATH=\"$dir:\$PATH\"' >> ~/.zshrc" ;;
esac
