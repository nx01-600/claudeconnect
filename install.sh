#!/bin/sh
# Installs claudeconnect on macOS or Linux: downloads the latest release and registers it
# with Claude Code.
#   curl -fsSL https://raw.githubusercontent.com/nx01-600/claudeconnect/main/install.sh | sh
set -eu
repo=nx01-600/claudeconnect

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) asset=claudeconnect-macos-aarch64 ;;
  Darwin-x86_64) asset=claudeconnect-macos-x86_64 ;;
  Linux-x86_64) asset=claudeconnect-linux-x86_64 ;;
  *) echo "No prebuilt binary for $(uname -s) $(uname -m). Build from source: cargo install --git https://github.com/$repo"; exit 1 ;;
esac

command -v claude >/dev/null 2>&1 || { echo "Claude Code (claude) is not on PATH. Install it first: https://code.claude.com"; exit 1; }

dir="$HOME/.claudeconnect/bin"
mkdir -p "$dir"
echo "Downloading $asset"
curl -fsSL "https://github.com/$repo/releases/latest/download/$asset" -o "$dir/claudeconnect.download"
chmod +x "$dir/claudeconnect.download"
mv -f "$dir/claudeconnect.download" "$dir/claudeconnect"

"$dir/claudeconnect" install
