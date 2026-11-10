#!/bin/sh
# Fetch the CI build of a main commit into builds/ with a sortable name:
#   builds/OpenCircle-<last release>-D<n>-<sha7>.exe   (Windows)
#   builds/opencircle-<last release>-D<n>-<sha7>       (Linux)
# n = commits on main since that release, zero-padded (D050 sorts before D061). Older dev builds are deleted.
# Usage: scripts/dev-build.sh [<ci run id>]   (default: newest green CI run on main)
set -eu
cd "$(dirname "$0")/.."
run=${1:-$(gh run list -b main -w ci -s success -L 1 --json databaseId -q '.[0].databaseId')}
sha=$(gh run view "$run" --json headSha -q .headSha | cut -c1-7)
rel=$(git describe --tags --abbrev=0 --match 'v*' "$sha" | sed 's/^v//')
n=$(printf %03d "$(git rev-list --count "v$rel..$sha")")
tmp=$(mktemp -d)
gh run download "$run" -n opencircle-windows-exe -D "$tmp/win"
gh run download "$run" -n opencircle-linux-bin -D "$tmp/linux"
mkdir -p builds
rm -f builds/OpenCircle-*-D*-*.exe builds/opencircle-*-D*-*
install -m 644 "$tmp/win/opencircle.exe" "builds/OpenCircle-$rel-D$n-$sha.exe"
install -m 755 "$tmp/linux/opencircle" "builds/opencircle-$rel-D$n-$sha"
rm -rf "$tmp"
ls builds/
