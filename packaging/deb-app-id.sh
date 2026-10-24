#!/usr/bin/env bash
# deb-app-id.sh <file.deb>: renames the package's desktop file and icons from the
# binary's name (what cargo-packager writes) to the app id, which GNOME's portals
# look up: com.opencircle.app.desktop and com.opencircle.app.png. Adds the
# Recommends line (WebKitGTK for the results card). Rewrites in place.
set -euo pipefail
deb=$1
id=com.opencircle.app
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
dpkg-deb -R "$deb" "$dir"
mv "$dir/usr/share/applications/opencircle.desktop" "$dir/usr/share/applications/$id.desktop"
for icon in "$dir"/usr/share/icons/hicolor/*/apps/opencircle.png; do
  mv "$icon" "${icon%/*}/$id.png"
done
# The results card's web view (opencircle-sheet) is optional: without it results
# open in the browser. cargo-packager can't write Recommends, so it goes in here.
if ! grep -q '^Recommends:' "$dir/DEBIAN/control"; then
  sed -i '/^Depends:/a Recommends: libwebkit2gtk-4.1-0' "$dir/DEBIAN/control"
fi
if [ -f "$dir/DEBIAN/md5sums" ]; then
  sed -i "s#/applications/opencircle\.desktop\$#/applications/$id.desktop#; s#/apps/opencircle\.png\$#/apps/$id.png#" "$dir/DEBIAN/md5sums"
fi
dpkg-deb --root-owner-group -Zxz -b "$dir" "$deb"
list=$(dpkg-deb -c "$deb")
if ! grep -q "usr/share/applications/$id\.desktop$" <<<"$list" \
  || grep -q '/opencircle\.desktop$' <<<"$list"; then
  echo "$deb: desktop file not renamed to $id.desktop" >&2
  exit 1
fi
if ! dpkg-deb -f "$deb" Recommends | grep -q libwebkit2gtk-4.1-0; then
  echo "$deb: no Recommends: libwebkit2gtk-4.1-0" >&2
  exit 1
fi
