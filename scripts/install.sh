#!/bin/sh
# OpenCircle for Linux, one command, no sudo:
#
#   curl -fsSL https://raw.githubusercontent.com/AmerAlreyahi/OpenCircle/main/scripts/install.sh | sh
#   ... | sh -s -- --uninstall
#
# Downloads the latest release's AppImage, checks it, puts it in ~/.local/bin, adds
# the app-menu entry and icon (app id com.opencircle.app), and starts it. Checks, in
# this order, and stops at the first that fails (nothing is installed then):
#   1. the minisign signature over SHA256SUMS.txt, made with the release key the
#      app's own updater trusts, with the trusted comment "OpenCircle v<tag>";
#   2. the AppImage's SHA-256 against that signed list.
# Needs: curl or wget, openssl 3. Safe to run again (it upgrades in place).
set -eu

REPO="AmerAlreyahi/OpenCircle"
APP_ID="com.opencircle.app"
# The release signing key (minisign public key). The same line is verify.rs's
# RELEASE_PUBLIC_KEY; a test keeps the two equal.
KEY="RWSbS2U2Cjix7vMbkgnr+jEZI2VKl7+HWo2Scq4uImBxKNgXCPo+K5oY"
# DER header of an Ed25519 public key (SubjectPublicKeyInfo), base64.
SPKI="MCowBQYDK2VwAyEA"

# Test mode (the script's own test): another key, another place to download from,
# no app start. Off unless asked for, and loud when on.
if [ "${OPENCIRCLE_INSTALL_TEST:-}" = 1 ]; then
  KEY=${OPENCIRCLE_KEY:-$KEY}
  echo "OpenCircle install: TEST MODE (key, download place and start are overridden)" >&2
else
  OPENCIRCLE_BASE="" OPENCIRCLE_TAG="" OPENCIRCLE_NO_START=""
fi

say() { printf '%s\n' "$*"; }
# Every failure says what happened, what is safe, and what to do next.
die() {
  printf 'OpenCircle install: %s\n' "$*" >&2
  exit 1
}

usage() {
  say "Usage: install.sh [--uninstall]"
  say "  (no option)   install or upgrade OpenCircle in ~/.local/bin and start it"
  say "  --uninstall   remove what this script installed (your settings stay)"
}

HOME_DIR=${HOME:-}
[ -n "$HOME_DIR" ] || die "HOME is not set, so there is no place to install to. Set HOME and run this again."
BIN_DIR="$HOME_DIR/.local/bin"
BIN="$BIN_DIR/opencircle"
DATA="${XDG_DATA_HOME:-$HOME_DIR/.local/share}"
DESKTOP="$DATA/applications/$APP_ID.desktop"
ICON="$DATA/icons/hicolor/128x128/apps/$APP_ID.png"
AUTOSTART="${XDG_CONFIG_HOME:-$HOME_DIR/.config}/autostart/$APP_ID.desktop"
MARK="X-OpenCircle-Generated=true"

# Stop a running copy through its own command (no running copy is fine).
stop_app() {
  [ -x "$BIN" ] && "$BIN" quit >/dev/null 2>&1 || true
}

uninstall() {
  found=0
  stop_app
  for f in "$BIN" "$ICON" "$DESKTOP" "$AUTOSTART"; do
    [ -e "$f" ] || continue
    # A desktop entry we did not write (a package's, or the user's) stays.
    case $f in
      "$DESKTOP" | "$AUTOSTART") grep -q -e "$MARK" -e "$BIN" "$f" 2>/dev/null || continue ;;
    esac
    rm -f "$f"
    say "Removed $f"
    found=1
  done
  [ "$found" = 1 ] || say "Nothing to remove: OpenCircle is not installed by this script."
  say "Your settings in ~/.config/opencircle are kept; delete that folder to remove them too."
}

case ${1:-} in
  "") ;;
  --uninstall) uninstall; exit 0 ;;
  -h | --help) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac

[ "$(uname -s)" = Linux ] || die "this installer is for Linux. Windows has an installer on the release page."
case $(uname -m) in
  x86_64 | amd64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) die "no OpenCircle build for $(uname -m). Releases: https://github.com/$REPO/releases" ;;
esac
command -v openssl >/dev/null 2>&1 || die "openssl is needed to check the download and is not installed. Install it (for example: sudo apt install openssl) and run this again."
case $(openssl version 2>/dev/null) in
  "OpenSSL "[3-9]*) ;;
  *) die "this needs OpenSSL 3 or newer to check the signature; found: $(openssl version 2>/dev/null). Nothing was installed." ;;
esac

TMP=$(mktemp -d) || die "could not make a temporary folder."
trap 'rm -rf "$TMP"' EXIT
trap 'exit 1' INT TERM HUP

# fetch URL DEST
fetch() {
  case $1 in
    file://*)
      [ "${OPENCIRCLE_INSTALL_TEST:-}" = 1 ] || die "refusing a non-https download."
      cp "${1#file://}" "$2" 2>/dev/null
      ;;
    https://*)
      if command -v curl >/dev/null 2>&1; then
        curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
      elif command -v wget >/dev/null 2>&1; then
        wget -q --https-only -O "$2" "$1"
      else
        die "neither curl nor wget is installed. Install one (for example: sudo apt install curl) and run this again."
      fi
      ;;
    *) die "refusing a non-https download." ;;
  esac
}
download() {
  fetch "$1" "$2" || die "could not download $1. Nothing was installed; check your connection and run this again."
}

# slice FILE SKIP COUNT: bytes of a file. hex: stdin as lowercase hex.
slice() { dd if="$1" bs=1 skip="$2" count="$3" 2>/dev/null; }
hex() { od -An -v -tx1 | tr -d ' \n'; }
size() { wc -c <"$1" | tr -d ' '; }
b64() { openssl base64 -d -A 2>/dev/null; }

# verify_sums SUMS SIG TAG: the signature over SUMS is ours, for this release.
verify_sums() {
  sums=$1 sig=$2 tag=$3
  forged="the signature on this download is not valid, so it is not from the OpenCircle release process. Nothing was installed. Try again later; if it keeps happening, tell us at https://github.com/$REPO/issues"
  printf '%s' "$KEY" | b64 >"$TMP/key.bin" || die "internal error: bad release key."
  [ "$(size "$TMP/key.bin")" = 42 ] && [ "$(slice "$TMP/key.bin" 0 2 | hex)" = 4564 ] || die "internal error: bad release key."
  sed -n 2p "$sig" | tr -d '\r\n' | b64 >"$TMP/sig.bin" || die "$forged"
  [ "$(size "$TMP/sig.bin")" = 74 ] || die "$forged"
  [ "$(slice "$TMP/sig.bin" 2 8 | hex)" = "$(slice "$TMP/key.bin" 2 8 | hex)" ] || die "$forged"
  { printf '%s' "$SPKI" | b64; slice "$TMP/key.bin" 10 32; } >"$TMP/pk.der"
  slice "$TMP/sig.bin" 10 64 >"$TMP/sig64"
  case $(slice "$TMP/sig.bin" 0 2 | hex) in
    4564) msg=$sums ;;  # "Ed": the file itself is signed
    4544) # "ED": its BLAKE2b-512 is signed
      openssl dgst -blake2b512 -binary "$sums" >"$TMP/msg" 2>/dev/null || die "$forged"
      msg="$TMP/msg"
      ;;
    *) die "$forged" ;;
  esac
  ed25519() { openssl pkeyutl -verify -pubin -inkey "$TMP/pk.der" -keyform DER -rawin -in "$1" -sigfile "$2" >/dev/null 2>&1; }
  ed25519 "$msg" "$TMP/sig64" || die "$forged"
  # The signed ("trusted") comment names the release; the global signature covers it.
  comment=$(sed -n 's/^trusted comment: //p' "$sig" | tr -d '\r' | head -n 1)
  sed -n 4p "$sig" | tr -d '\r\n' | b64 >"$TMP/gsig" || die "$forged"
  [ "$(size "$TMP/gsig")" = 64 ] || die "$forged"
  { cat "$TMP/sig64"; printf '%s' "$comment"; } >"$TMP/gmsg"
  ed25519 "$TMP/gmsg" "$TMP/gsig" || die "$forged"
  [ "$comment" = "OpenCircle $tag" ] ||
    die "the signature is for \"$comment\", not $tag: an old release is being served as the new one. Nothing was installed."
}

# --- the release -------------------------------------------------------------
TAG=${OPENCIRCLE_TAG:-}
if [ -z "$TAG" ]; then
  download "https://api.github.com/repos/$REPO/releases/latest" "$TMP/latest.json"
  TAG=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$TMP/latest.json" | head -n 1)
fi
case $TAG in
  v[0-9]*.[0-9]*.[0-9]*) ;;
  *) die "could not tell which release is the latest. Nothing was installed; try again in a minute, or download it from https://github.com/$REPO/releases" ;;
esac
case $TAG in *[!A-Za-z0-9.+-]*) die "unexpected release name \"$TAG\". Nothing was installed." ;; esac
BASE=${OPENCIRCLE_BASE:-https://github.com/$REPO/releases/download/$TAG}

say "Installing OpenCircle $TAG ..."
download "$BASE/SHA256SUMS.txt" "$TMP/SHA256SUMS.txt"
download "$BASE/SHA256SUMS.txt.minisig" "$TMP/SHA256SUMS.txt.minisig"
verify_sums "$TMP/SHA256SUMS.txt" "$TMP/SHA256SUMS.txt.minisig" "$TAG"
say "Signature OK (release key)."

# The AppImage for this machine, as the signed list names it.
ASSET=$(awk -v s="_$ARCH.AppImage" '{ n = $2; sub(/^\*/, "", n); if (length(n) > length(s) && substr(n, length(n) - length(s) + 1) == s) print n }' "$TMP/SHA256SUMS.txt")
case $ASSET in
  "" | *"
"*) die "the release has no single $ARCH AppImage. Nothing was installed. Releases: https://github.com/$REPO/releases" ;;
  *[!A-Za-z0-9._-]*) die "unexpected file name in the release. Nothing was installed." ;;
esac
WANT=$(awk -v n="$ASSET" '{ f = $2; sub(/^\*/, "", f); if (f == n) { print $1; exit } }' "$TMP/SHA256SUMS.txt")
case $WANT in
  *[!0-9a-f]* | "") die "the signed list has no checksum for $ASSET. Nothing was installed." ;;
esac
[ "${#WANT}" = 64 ] || die "the signed list has no valid checksum for $ASSET. Nothing was installed."

download "$BASE/$ASSET" "$TMP/$ASSET"
GOT=$(openssl dgst -sha256 -r "$TMP/$ASSET" | cut -d ' ' -f 1)
[ "$GOT" = "$WANT" ] || die "the download's checksum does not match the signed list (it may be damaged or tampered with). Nothing was installed. Run this again; if it repeats, tell us at https://github.com/$REPO/issues"
say "Checksum OK."

# --- install -----------------------------------------------------------------
chmod 755 "$TMP/$ASSET"
mkdir -p "$BIN_DIR" "$DATA/applications" "${ICON%/*}" || die "could not create the folders under $HOME_DIR/.local. Nothing was installed."
stop_app
# Same folder, then rename: a running copy and a half-copied file never meet.
if ! { cp "$TMP/$ASSET" "$BIN.new" && mv -f "$BIN.new" "$BIN"; }; then
  rm -f "$BIN.new"
  die "could not write $BIN. Nothing was changed."
fi

# The icon is inside the AppImage; a missing one only costs the menu picture.
if (cd "$TMP" && "$TMP/$ASSET" --appimage-extract "usr/share/icons/hicolor/128x128/apps/opencircle.png" >/dev/null 2>&1) &&
  [ -f "$TMP/squashfs-root/usr/share/icons/hicolor/128x128/apps/opencircle.png" ]; then
  cp "$TMP/squashfs-root/usr/share/icons/hicolor/128x128/apps/opencircle.png" "$ICON"
else
  say "Note: could not read the icon from the download; the menu entry has no picture."
fi

# Exec path in the desktop-entry quoting: plain characters only here, else refuse
# (the app's own entry writer quotes the rest; this script has no need to).
case $BIN in
  *[!A-Za-z0-9/._+-]*) die "the install path $BIN has characters a menu entry cannot hold safely. Set HOME to a plainer path, or move OpenCircle yourself. The app is installed there already." ;;
esac
cat >"$DESKTOP" <<EOF
[Desktop Entry]
# Written by the OpenCircle installer. Remove with: install.sh --uninstall
Type=Application
Name=OpenCircle
Comment=Circle anything on screen to search, read, translate or ask about it
Exec=$BIN
Icon=$APP_ID
StartupWMClass=$APP_ID
Terminal=false
Categories=Utility;
$MARK
EOF
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "${DESKTOP%/*}" >/dev/null 2>&1 || true

say "Installed $BIN"
case ":${PATH:-}:" in
  *":$BIN_DIR:"*) ;;
  *) say "Note: $BIN_DIR is not on your PATH. Add it to run \"opencircle\" from a terminal; the app menu entry works already." ;;
esac

# --- start -------------------------------------------------------------------
if [ -n "${OPENCIRCLE_NO_START:-}" ]; then
  exit 0
fi
if [ -z "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]; then
  say "There is no desktop session here, so it was not started. Start OpenCircle from the app menu."
  exit 0
fi
if command -v setsid >/dev/null 2>&1; then
  setsid "$BIN" </dev/null >/dev/null 2>&1 &
else
  "$BIN" </dev/null >/dev/null 2>&1 &
fi
sleep 2
if kill -0 $! 2>/dev/null; then
  say "OpenCircle is running. Press Ctrl+Shift+Space to use it, or open it from the app menu any time."
else
  say "It did not stay open. Run \"$BIN\" in a terminal to see why."
fi
