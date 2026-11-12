#!/usr/bin/env bash
# Test: regenerating the v0.1.7 manifests reproduces the committed files, they pass the
# schemas, and a corrupted manifest is rejected. Needs the committed files (git).
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/store-manifests.sh 0.1.7 2026-11-12 >/dev/null
git diff --exit-code -- packaging/stores/winget packaging/stores/scoop
python3 -I packaging/stores/validate.py >/dev/null
bad=$(mktemp -d); trap 'rm -rf "$bad"' EXIT
mkdir "$bad/0.1.7"; cp packaging/stores/winget/0.1.7/* "$bad/0.1.7/"
sed -i 's/^InstallerSha256: .*/&/; s/  InstallerSha256: .*/  InstallerSha256: nothex/' "$bad/0.1.7/AmerAlreyahi.OpenCircle.installer.yaml"
if python3 -I packaging/stores/validate.py "$bad/0.1.7" >/dev/null; then echo "a bad hash passed the schema"; exit 1; fi
# The NEXT release's form (publisher, switches, updater marker): made from made-up checksums
# into a temp folder, schema-checked, and the points that matter must be in it.
new=$(mktemp -d); trap 'rm -rf "$bad" "$new"' EXIT
a=$(printf a | sha256sum | cut -d' ' -f1); b=$(printf b | sha256sum | cut -d' ' -f1)
printf '%s  OpenCircle-Setup-9.9.9-x64.exe\n%s  OpenCircle-9.9.9-x64-portable.exe\n' "$a" "$b" > "$new/sums"
SUMS="$new/sums" STORES_OUT="$new/out" scripts/store-manifests.sh 9.9.9 2030-01-01 >/dev/null
SCOOP_JSON="$new/out/scoop/opencircle.json" python3 -I packaging/stores/validate.py "$new/out/winget/9.9.9" >/dev/null
i="$new/out/winget/9.9.9/AmerAlreyahi.OpenCircle.installer.yaml"
for want in 'Publisher: Amer Alreyahi' 'InstallLocation: /dir="<INSTALLPATH>"' 'InstallerReturnCode: 1641'; do
  grep -qF -- "$want" "$i" || { echo "missing in the new installer manifest: $want"; exit 1; }
done
grep -qF 'opencircle.managed' "$new/out/scoop/opencircle.json" || { echo "the Scoop manifest lacks the updater marker"; exit 1; }
echo "store manifests: ok"
