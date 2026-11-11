#!/usr/bin/env bash
# Refresh the About page's developer card from the owner's public GitHub profile:
# assets/about/profile.toml (name, login, website; no company or location, by the owner's choice) and
# assets/about/avatar.png (128 px). Public data, no secrets. The bio is written
# by hand and kept: this script never overwrites it.
set -euo pipefail
cd "$(dirname "$0")/.."
user=AmerAlreyahi
out=assets/about/profile.toml
bio="AI and PPM solution architect building fast, private desktop tools."
if [ -f "$out" ]; then
  old=$(sed -n 's/^bio = "\(.*\)"$/\1/p' "$out")
  [ -n "$old" ] && bio=$old
fi
mkdir -p assets/about
field() { gh api "users/$user" --jq "$1 // \"\"" | tr -d '\r\n"\\'; }
name=$(field .name)
login=$(field .login)
blog=$(field .blog)
case "$blog" in http*|"") ;; *) blog="https://$blog" ;; esac
cat > "$out" <<TOML
# Written by scripts/refresh-about.sh from https://github.com/$user (the bio is hand-written and kept).
name = "$name"
login = "$login"
website = "$blog"
bio = "$bio"
TOML
# GitHub resizes avatars itself (s=128) and serves a PNG or a JPEG (JPEG since 2026-11); the file keeps
# the name avatar.png and the app sniffs its content. Accept only real image bytes, else keep the committed
# avatar (a bad download must never reach a release; never fail the release for an avatar).
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
if curl -fsSL "https://avatars.githubusercontent.com/u/80674857?v=4&s=128" -o "$tmp"; then
  magic=$(head -c 3 "$tmp" | od -An -tx1 | tr -d ' \n')
  case "$magic" in
    89504e|ffd8ff) cp "$tmp" assets/about/avatar.png ;;
    *) echo "::warning::refresh-about: the avatar download is not a PNG or JPEG; kept the committed one" ;;
  esac
else
  echo "::warning::refresh-about: the avatar download failed; kept the committed one"
fi
