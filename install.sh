#!/bin/sh
# Installs the latest mdd release.
#   curl -fsSL https://mddog.pages.dev/install.sh | sh
# MDDOG_INSTALL_DIR picks the folder (default ~/.local/bin).
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

case $(uname -s) in
  Darwin) os=macos ;;
  Linux) os=linux ;;
  *) fail "mddog runs only on macOS and Linux" ;;
esac
case $(uname -m) in
  x86_64 | amd64) arch=x86_64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) fail "no mddog build for $(uname -m)" ;;
esac
# A shell under Rosetta reports x86_64 on Apple silicon, so ask the hardware too.
[ "$os" = macos ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ] && arch=arm64
if command -v sha256sum >/dev/null; then
  check() { sha256sum -c "$1"; }
else
  check() { shasum -a 256 -c "$1"; }
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"
tarball=mdd-$os-$arch.tar.gz
curl -fsSL -o "$tarball" "$url/$tarball" || fail "could not download $url/$tarball"
curl -fsSL -o "$tarball.sha256" "$url/$tarball.sha256" || fail "could not download the checksum"
check "$tarball.sha256" >/dev/null || fail "checksum mismatch"
tar xzf "$tarball"

mkdir -p "$dir"
new=$(mktemp "$dir/.mdd.XXXXXX") || fail "could not write to $show"
{ cat mdd > "$new" && chmod 755 "$new" && mv -f "$new" "$dir/mdd"; } || { rm -f "$new"; fail "could not install to $show/mdd"; }
[ "$os" = macos ] && { xattr -d com.apple.quarantine "$dir/mdd" 2>/dev/null || true; }
echo "$ok Installed $("$dir/mdd" --version) to $show/mdd"

echo
case ":$PATH:" in
  *":$dir:"*) ;;
  *)
    login=${SHELL:-}
    [ -n "$login" ] || { [ "$os" = linux ] && login=bash || login=zsh; }
    case "${login##*/}" in
      fish) add="fish_add_path $show" ;;
      bash) [ "$os" = macos ] && rc=.bash_profile || rc=.bashrc; add="echo 'export PATH=\"$rc_dir:\$PATH\"' >> ~/$rc" ;;
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

To update mddog, run the same install command again.
EOF
