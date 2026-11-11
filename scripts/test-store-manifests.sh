#!/usr/bin/env bash
# Test: regenerating the v0.1.6 manifests reproduces the committed files, they pass the
# schemas, and a corrupted manifest is rejected. Needs the committed files (git).
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/store-manifests.sh 0.1.6 2026-11-11 >/dev/null
git diff --exit-code -- packaging/stores/winget packaging/stores/scoop
python3 -I packaging/stores/validate.py >/dev/null
bad=$(mktemp -d); trap 'rm -rf "$bad"' EXIT
mkdir "$bad/0.1.6"; cp packaging/stores/winget/0.1.6/* "$bad/0.1.6/"
sed -i 's/^InstallerSha256: .*/&/; s/  InstallerSha256: .*/  InstallerSha256: nothex/' "$bad/0.1.6/AmerAlreyahi.OpenCircle.installer.yaml"
if python3 -I packaging/stores/validate.py "$bad/0.1.6" >/dev/null; then echo "a bad hash passed the schema"; exit 1; fi
echo "store manifests: ok"
