#!/bin/sh
# Installs the latest mdd release and has macOS start a hidden mdd at login, so the first preview opens fast.
#   curl -fsSL https://raw.githubusercontent.com/qu8n/mddog/main/install.sh | sh
# MDDOG_INSTALL_DIR picks the folder (default ~/.local/bin). MDDOG_NO_START_AT_LOGIN=1 skips the start at login.
set -eu

url=${MDDOG_RELEASE_URL:-https://github.com/qu8n/mddog/releases/latest/download}
dir=${MDDOG_INSTALL_DIR:-$HOME/.local/bin}

# Color only on a terminal, and never with NO_COLOR set (no-color.org).
ok='✓' err='error:'
if [ -z "${NO_COLOR:-}" ]; then
  if [ -t 1 ]; then ok=$(printf '\033[32m✓\033[0m'); fi
  if [ -t 2 ]; then err=$(printf '\033[31merror:\033[0m'); fi
fi
fail() { echo "mddog install: $err $*" >&2; exit 1; }

# Show ~ for the home folder, and put $HOME in the line people paste.
case $dir in
  "$HOME"/*) show="~${dir#"$HOME"}" rc_dir="\$HOME${dir#"$HOME"}" ;;
  *) show=$dir rc_dir=$dir ;;
esac

[ "$(uname -s)" = Darwin ] || fail "mddog runs only on macOS"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"
# A shell under Rosetta reports x86_64 on Apple silicon, so ask the hardware too.
arch=$(uname -m)
[ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ] && arch=arm64
tarball=mdd-macos-$arch.tar.gz
curl -fsSL -o "$tarball" "$url/$tarball" || fail "could not download $url/$tarball"
curl -fsSL -o "$tarball.sha256" "$url/$tarball.sha256" || fail "could not download the checksum"
shasum -a 256 -c "$tarball.sha256" >/dev/null || fail "checksum mismatch"
tar xzf "$tarball"

mkdir -p "$dir"
mv -f mdd "$dir/mdd"
xattr -d com.apple.quarantine "$dir/mdd" 2>/dev/null || true
echo "$ok Installed $("$dir/mdd" --version) to $show/mdd"

# The launchd job records the binary's path, so it runs after the move. Its errors still reach stderr.
[ "${MDDOG_NO_START_AT_LOGIN:-}" = 1 ] || "$dir/mdd" --start-at-login >/dev/null || [ $? -eq 3 ]

echo
case ":$PATH:" in
  *":$dir:"*) ;;
  *)
    case "${SHELL##*/}" in
      fish) add="fish_add_path $show" ;;
      bash) add="echo 'export PATH=\"$rc_dir:\$PATH\"' >> ~/.bash_profile" ;;
      *) add="echo 'export PATH=\"$rc_dir:\$PATH\"' >> ~/.zshrc" ;;
    esac
    echo "$show is not on your PATH. To add it, run this, then open a new terminal:"
    echo
    echo "    $add"
    echo
    ;;
esac
cat <<'EOF'
Get started:

    mdd             pick a Markdown file to preview
    mdd FILE.md     preview one file
    mdd --help      see all options
EOF
