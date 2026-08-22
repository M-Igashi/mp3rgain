#!/usr/bin/env bash
#
# Build the mp3rgui Windows installer with Inno Setup.
#
# Runs on a Windows runner (Git Bash). Both the CI smoke check and the release
# workflow call this, so an .iss syntax error surfaces on an ordinary push
# rather than in the middle of cutting a release.
#
# Usage: scripts/build-windows-installer.sh <version> <output-base> <x64-exe> <arm64-exe>

set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <version> <output-base> <x64-exe> <arm64-exe>" >&2
  exit 2
fi

version="$1"
output_base="$2"
x64_exe="$3"
arm64_exe="$4"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Stage under target/ (already gitignored) so a local run leaves no build
# output inside packages/.
staging="$repo_root/target/windows-installer"
rm -rf "$staging"
mkdir -p "$staging/x86_64" "$staging/arm64"
cp "$repo_root/packages/windows/mp3rgui.iss" "$staging/"
cp "$x64_exe" "$staging/x86_64/mp3rgui.exe"
cp "$arm64_exe" "$staging/arm64/mp3rgui.exe"
cp "$repo_root/mp3rgui/icons/icon.ico" "$staging/icon.ico"
cp "$repo_root/LICENSE" "$staging/LICENSE.txt"

# GitHub's Windows images install Inno Setup here; fall back to PATH so a
# local run with a different install location still works.
iscc="/c/Program Files (x86)/Inno Setup 6/ISCC.exe"
if [ ! -x "$iscc" ]; then
  iscc="$(command -v iscc)"
fi

cd "$staging"
"$iscc" "/DMyAppVersion=$version" "/DOutputBase=$output_base" mp3rgui.iss

test -f "$output_base.exe"
echo "built $staging/$output_base.exe"
