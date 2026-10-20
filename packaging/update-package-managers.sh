#!/usr/bin/env bash
# Writes the Homebrew cask and the winget manifest for a published GitHub Release.
#
#   bash packaging/update-package-managers.sh 0.2.0
#
# It downloads the release's .dmg and .msi, and writes:
#   packaging/homebrew/anvilcad.rb                        (copy to Casks/ in swperb/homebrew-tap)
#   packaging/winget/manifests/a/AnvilCAD/AnvilCAD/<v>/   (submit to microsoft/winget-pkgs)
#
# Needs: gh, shasum, and msiinfo (msitools: brew install msitools / apt install msitools)
# to read the .msi's ProductCode. Run it after the release workflow has published the tag,
# commit the result, then publish both (see packaging/README.md).
set -euo pipefail

version="${1:?usage: update-package-managers.sh VERSION}"
version="${version#v}"
tag="v$version"
repo="swperb/anvilcad"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

dmg="AnvilCAD-$version-macos-universal.dmg"
msi="AnvilCAD-$version-windows-x64.msi"
base="https://github.com/$repo/releases/download/$tag"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
gh release download "$tag" -R "$repo" -D "$tmp" -p "$dmg" -p "$msi"

dmg_sha="$(shasum -a 256 "$tmp/$dmg" | cut -d' ' -f1)"
msi_sha="$(shasum -a 256 "$tmp/$msi" | cut -d' ' -f1 | tr '[:lower:]' '[:upper:]')"
product_code="$(msiinfo export "$tmp/$msi" Property | awk -F'\t' '$1 == "ProductCode" { print $2 }' | tr -d '\r')"
upgrade_code="$(msiinfo export "$tmp/$msi" Property | awk -F'\t' '$1 == "UpgradeCode" { print $2 }' | tr -d '\r')"
release_date="$(gh release view "$tag" -R "$repo" --json publishedAt -q '.publishedAt[:10]')"
[ -n "$product_code" ] && [ -n "$upgrade_code" ] || { echo "could not read the MSI's codes" >&2; exit 1; }

mkdir -p packaging/homebrew
cat >packaging/homebrew/anvilcad.rb <<EOF
# Homebrew cask for AnvilCAD, written by packaging/update-package-managers.sh.
#
#   brew install --cask swperb/tap/anvilcad
#
# Published in https://github.com/swperb/homebrew-tap. The app is signed with a Developer ID
# and notarized, so it opens without a Gatekeeper prompt.
cask "anvilcad" do
  version "$version"
  sha256 "$dmg_sha"

  url "https://github.com/$repo/releases/download/v#{version}/AnvilCAD-#{version}-macos-universal.dmg"
  name "AnvilCAD"
  desc "Free and open-source parametric 3D CAD"
  homepage "https://github.com/$repo"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on :macos

  app "AnvilCAD.app"
  binary "#{appdir}/AnvilCAD.app/Contents/MacOS/anvilcad-agent"
  binary "#{appdir}/AnvilCAD.app/Contents/MacOS/anvil-bulk"

  zap trash: "~/.config/anvilcad"
end
EOF

dir="packaging/winget/manifests/a/AnvilCAD/AnvilCAD/$version"
mkdir -p "$dir"
schema="1.12.0"
id="AnvilCAD.AnvilCAD"

cat >"$dir/$id.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
EOF

cat >"$dir/$id.installer.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $version
InstallerType: msi
Scope: machine
InstallModes:
- interactive
- silent
- silentWithProgress
UpgradeBehavior: install
Commands:
- anvilcad
- anvilcad-agent
- anvil-bulk
- anvil-migrate
ReleaseDate: $release_date
AppsAndFeaturesEntries:
- ProductCode: '$product_code'
  UpgradeCode: '$upgrade_code'
Installers:
- Architecture: x64
  InstallerUrl: $base/$msi
  InstallerSha256: $msi_sha
  ProductCode: '$product_code'
ManifestType: installer
ManifestVersion: $schema
EOF

cat >"$dir/$id.locale.en-US.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $version
PackageLocale: en-US
Publisher: AnvilCAD contributors
PublisherUrl: https://github.com/$repo
PublisherSupportUrl: https://github.com/$repo/issues
PackageName: AnvilCAD
PackageUrl: https://github.com/$repo
License: MIT OR Apache-2.0
LicenseUrl: https://github.com/$repo/blob/main/LICENSE-MIT
ShortDescription: Free and open-source parametric 3D CAD with its own geometry kernels.
Description: |-
  AnvilCAD is free, open-source parametric 3D CAD written in Rust. It has a feature tree,
  sketches with relations and dimensions, extrudes, revolves, fillets and patterns on an
  exact B-rep kernel, STEP import and export, and tools to migrate SOLIDWORKS parts with
  their feature trees.
Moniker: anvilcad
Tags:
- 3d
- cad
- engineering
- parametric
- step
ReleaseNotesUrl: https://github.com/$repo/releases/tag/$tag
ManifestType: defaultLocale
ManifestVersion: $schema
EOF

echo "wrote packaging/homebrew/anvilcad.rb and $dir"
