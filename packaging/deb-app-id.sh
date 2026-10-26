#!/usr/bin/env bash
# deb-app-id.sh <file.deb>: renames the package's desktop file and icons from the
# binary's name (what cargo-packager writes) to the app id, which GNOME's portals
# look up: com.opencircle.app.desktop and com.opencircle.app.png. Rewrites in place.
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
