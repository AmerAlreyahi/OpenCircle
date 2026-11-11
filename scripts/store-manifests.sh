#!/usr/bin/env bash
# Regenerate the winget and Scoop manifests for a PUBLISHED release from its
# SHA256SUMS.txt (the only network use: `gh release download` of that file).
# Usage: scripts/store-manifests.sh <version> [release-date YYYY-MM-DD]
#   e.g. scripts/store-manifests.sh 0.1.6 2026-11-11
# Writes packaging/stores/winget/<version>/ and packaging/stores/scoop/opencircle.json.
# With SUMS=<file> the checksum file is read from disk instead (offline test).
set -euo pipefail
cd "$(dirname "$0")/.."
ver=${1:?usage: store-manifests.sh <version> [release-date]}
date=${2:-}
[[ $ver =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version must be X.Y.Z" >&2; exit 2; }
repo=AmerAlreyahi/OpenCircle
id=AmerAlreyahi.OpenCircle
arp_publisher=opencircle # what the MSI writes as Publisher (measured on the Windows VM, 0.1.6)
publisher=OpenCircle
base=https://github.com/$repo/releases/download/v$ver
setup=OpenCircle-Setup-$ver-x64.exe
portable=OpenCircle-$ver-x64-portable.exe

sums=${SUMS:-}
if [ -z "$sums" ]; then
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  gh release download "v$ver" -R $repo -p SHA256SUMS.txt -D "$tmp"
  sums=$tmp/SHA256SUMS.txt
fi
hash_of() { # file name -> lowercase hash, exactly one line
  local h; h=$(awk -v f="$1" '$2==f {print tolower($1)}' "$sums" | sort -u)
  [ "$(printf '%s\n' "$h" | wc -l)" = 1 ] && [[ $h =~ ^[0-9a-f]{64}$ ]] || { echo "no unique hash for $1" >&2; exit 1; }
  printf %s "$h"
}
setup_hash=$(hash_of "$setup"); portable_hash=$(hash_of "$portable")

w=packaging/stores/winget/$ver
mkdir -p "$w" packaging/stores/scoop
schema=1.28.0
rd=${date:+"
  ReleaseDate: $date"}
cat > "$w/$id.yaml" <<Y
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $ver
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
Y
cat > "$w/$id.installer.yaml" <<Y
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $ver
InstallerType: exe
Scope: machine
InstallModes:
- silent
- silentWithProgress
InstallerSwitches:
  Silent: /quiet
  SilentWithProgress: /quiet
ExpectedReturnCodes:
- InstallerReturnCode: 1618
  ReturnResponse: installInProgress
- InstallerReturnCode: 1602
  ReturnResponse: cancelledByUser
- InstallerReturnCode: 1639
  ReturnResponse: invalidParameter
- InstallerReturnCode: 3010
  ReturnResponse: rebootRequiredToFinish
UpgradeBehavior: install
AppsAndFeaturesEntries:
- DisplayName: OpenCircle
  Publisher: $arp_publisher
  ProductCode: OpenCircle
ElevationRequirement: elevationRequired${date:+"
ReleaseDate: $date"}
Installers:
- Architecture: x64
  InstallerUrl: $base/$setup
  InstallerSha256: ${setup_hash^^}
ManifestType: installer
ManifestVersion: $schema
Y
cat > "$w/$id.locale.en-US.yaml" <<Y
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $ver
PackageLocale: en-US
Publisher: $publisher
PublisherUrl: https://github.com/AmerAlreyahi
PublisherSupportUrl: https://github.com/$repo/issues
PrivacyUrl: https://github.com/$repo/blob/main/docs/privacy.md
Author: OpenCircle contributors
PackageName: OpenCircle
PackageUrl: https://github.com/$repo
License: MIT
LicenseUrl: https://github.com/$repo/blob/main/LICENSE.md
Copyright: Copyright (c) OpenCircle contributors
ShortDescription: Circle anything on screen to search, read, translate or ask about it
Description: |-
  OpenCircle brings Android's Circle to Search to the desktop. Press a shortcut, the screen
  freezes, circle or drag over anything and search it (Google Lens), copy its text, translate it
  or ask an AI about it. A second mode snips and annotates screenshots. Pure Rust, one small
  program, no telemetry; nothing leaves the PC unless you choose an action that needs the network.
Moniker: opencircle
Tags:
- screenshot
- ocr
- translate
- circle-to-search
- snip
- lens
ReleaseNotesUrl: https://github.com/$repo/releases/tag/v$ver
Documentations:
- DocumentLabel: Getting started
  DocumentUrl: https://github.com/$repo/blob/main/docs/getting-started.md
ManifestType: defaultLocale
ManifestVersion: $schema
Y

cat > packaging/stores/scoop/opencircle.json <<J
{
    "version": "$ver",
    "description": "Circle anything on screen to search, read, translate or ask about it",
    "homepage": "https://github.com/$repo",
    "license": "MIT",
    "notes": [
        "OpenCircle keeps its settings in %APPDATA%\\\\opencircle and its models in %LOCALAPPDATA%\\\\opencircle (not in Scoop's folders).",
        "Scoop manages updates for this install: switch off 'Keep OpenCircle up to date' in Settings > General, or put '[update]' and 'auto = false' in %APPDATA%\\\\opencircle\\\\config.toml.",
        "Quit OpenCircle (tray icon > Quit) before 'scoop update opencircle'."
    ],
    "architecture": {
        "64bit": {
            "url": "$base/$portable#/opencircle.exe",
            "hash": "$portable_hash"
        }
    },
    "bin": "opencircle.exe",
    "shortcuts": [
        [
            "opencircle.exe",
            "OpenCircle"
        ]
    ],
    "checkver": "github",
    "autoupdate": {
        "architecture": {
            "64bit": {
                "url": "https://github.com/$repo/releases/download/v\$version/OpenCircle-\$version-x64-portable.exe#/opencircle.exe"
            }
        },
        "hash": {
            "url": "https://github.com/$repo/releases/download/v\$version/SHA256SUMS.txt"
        }
    }
}
J
echo "wrote $w and packaging/stores/scoop/opencircle.json"
