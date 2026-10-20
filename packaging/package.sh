#!/usr/bin/env bash
# Builds AnvilCAD in release mode and packages it for one platform into dist/.
#
#   bash packaging/package.sh macos   [VERSION]   universal .app in a .dmg, and a .tar.gz
#   bash packaging/package.sh linux   [VERSION]   x86_64 AppImage and .tar.gz
#   bash packaging/package.sh windows [VERSION]   x64 .msi and .zip
#
# VERSION defaults to the workspace version in Cargo.toml; a leading "v" is dropped, so a
# tag such as v0.2.0 can be passed as it is. The release workflow
# (.github/workflows/release.yml) runs this on each platform, and it runs the same way on a
# developer's machine. On macOS, setting ANVIL_MACOS_SIGN_IDENTITY and ANVIL_NOTARY_PROFILE
# signs the app with a Developer ID and notarizes it (see packaging/macos/sign.sh); without
# them it gets an ad-hoc signature, which Apple Silicon requires before it runs anything.
# The Windows and Linux packages are not signed.
#
# Needs: macos  - Xcode command-line tools and the aarch64-apple-darwin and
#                 x86_64-apple-darwin Rust targets
#        linux  - curl, and the GTK 3, xkbcommon and Wayland development packages
#        windows - the MSVC toolchain, and WiX Toolset 5 (dotnet tool install --global wix
#                  --version 5.0.2) for the .msi; without wix only the .zip is made
set -euo pipefail

platform="${1:-}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="${2:-}"
if [ -z "$version" ]; then
  version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' Cargo.toml)"
fi
version="${version#v}"

dist="$root/dist"
stage="$root/target/package"
rm -rf "$stage"
mkdir -p "$dist" "$stage"

# The binaries every platform ships. anvil-migrate drives SOLIDWORKS through COM, so it
# only does anything on Windows.
bins=(anvilcad anvilcad-agent anvil-bulk)
crates=(-p anvil-app -p anvil-bulk)

docs_into() {
  cp LICENSE-MIT LICENSE-APACHE README.md "$1/"
}

case "$platform" in
macos)
  export MACOSX_DEPLOYMENT_TARGET=11.0
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked --target "$t" "${crates[@]}"
  done

  . packaging/macos/sign.sh
  bundle_id=io.github.swperb.anvilcad

  # The plain binaries for the .tar.gz, each signed on its own. The command-line tools are
  # copied into the app as they are; the main binary is signed again there as part of the
  # bundle.
  name="anvilcad-$version-macos-universal"
  mkdir -p "$stage/$name"
  for b in "${bins[@]}"; do
    lipo -create -output "$stage/$name/$b" \
      "target/aarch64-apple-darwin/release/$b" "target/x86_64-apple-darwin/release/$b"
    macos_sign "$stage/$name/$b" "$bundle_id.$b"
  done
  docs_into "$stage/$name"

  app="$stage/dmg/AnvilCAD.app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  for b in "${bins[@]}"; do cp "$stage/$name/$b" "$app/Contents/MacOS/$b"; done
  sed "s/@VERSION@/$version/g" packaging/macos/Info.plist >"$app/Contents/Info.plist"
  cp packaging/icons/anvilcad.icns "$app/Contents/Resources/anvilcad.icns"
  macos_sign "$app"
  codesign --verify --deep --strict "$app"

  # Notarize the app and the plain binaries in one submission, then staple the ticket to
  # the app so it opens without a network check. Binaries outside a bundle cannot carry a
  # ticket; Gatekeeper looks theirs up online.
  if [ -n "$macos_profile" ]; then
    (cd "$stage" && zip -qry notarize.zip dmg/AnvilCAD.app "$name")
    macos_notarize "$stage/notarize.zip"
    xcrun stapler staple "$app"
  fi

  tar -C "$stage" -czf "$dist/$name.tar.gz" "$name"

  ln -s /Applications "$stage/dmg/Applications"
  dmg="$dist/AnvilCAD-$version-macos-universal.dmg"
  rm -f "$dmg"
  hdiutil create -volname "AnvilCAD $version" -srcfolder "$stage/dmg" -fs HFS+ \
    -format UDZO -ov "$dmg"
  if [ -n "$macos_identity" ]; then
    macos_sign "$dmg" "$bundle_id.dmg"
  fi
  if [ -n "$macos_profile" ]; then
    macos_notarize "$dmg"
    xcrun stapler staple "$dmg"
    spctl --assess --type open --context context:primary-signature -vv "$dmg"
    spctl --assess --type exec -vv "$app"
  fi
  ;;

linux)
  cargo build --release --locked "${crates[@]}"

  name="anvilcad-$version-linux-x86_64"
  mkdir -p "$stage/$name"
  for b in "${bins[@]}"; do cp "target/release/$b" "$stage/$name/"; done
  cp packaging/linux/anvilcad.desktop packaging/icons/anvilcad.png "$stage/$name/"
  docs_into "$stage/$name"
  tar -C "$stage" -czf "$dist/$name.tar.gz" "$name"

  # The AppImage carries the AnvilCAD binaries only. GTK, Wayland, X11 and the GPU drivers
  # come from the host, as they must for the drivers to match it.
  appdir="$stage/AnvilCAD.AppDir"
  mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/256x256/apps"
  for b in "${bins[@]}"; do cp "target/release/$b" "$appdir/usr/bin/"; done
  cp packaging/linux/AppRun "$appdir/AppRun"
  chmod +x "$appdir/AppRun"
  cp packaging/linux/anvilcad.desktop "$appdir/"
  cp packaging/linux/anvilcad.desktop "$appdir/usr/share/applications/"
  cp packaging/icons/anvilcad.png "$appdir/"
  cp packaging/icons/anvilcad.png "$appdir/usr/share/icons/hicolor/256x256/apps/"

  # appimagetool and the AppImage runtime, pinned by version and checksum.
  tools="$root/target/appimage-tools"
  mkdir -p "$tools"
  fetch() { # url sha256 out
    if [ ! -f "$3" ] || ! echo "$2  $3" | sha256sum -c --status; then
      curl -sSfL -o "$3" "$1"
    fi
    echo "$2  $3" | sha256sum -c
  }
  fetch https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage \
    ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0 "$tools/appimagetool"
  fetch https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64 \
    2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d "$tools/runtime-x86_64"
  chmod +x "$tools/appimagetool"

  # Extract-and-run needs no FUSE, so this also works in containers.
  ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tools/appimagetool" --no-appstream \
    --runtime-file "$tools/runtime-x86_64" "$appdir" "$dist/AnvilCAD-$version-linux-x86_64.AppImage"
  ;;

windows)
  cargo build --release --locked "${crates[@]}" -p anvil-migrate
  bins+=(anvil-migrate)

  name="anvilcad-$version-windows-x64"
  mkdir -p "$stage/$name"
  for b in "${bins[@]}"; do cp "target/release/$b.exe" "$stage/$name/"; done
  docs_into "$stage/$name"
  rm -f "$dist/$name.zip"
  (cd "$stage" && 7z a -tzip -bso0 "$dist/$name.zip" "$name")

  if command -v wix >/dev/null 2>&1; then
    # An MSI version is three numbers; drop any pre-release suffix such as -rc1.
    msi_version="${version%%-*}"
    win() { cygpath -w "$1"; }
    wix build -arch x64 -d "Version=$msi_version" -d "BinDir=$(win "$root/target/release")" \
      -d "Root=$(win "$root")" -o "$(win "$dist/AnvilCAD-$version-windows-x64.msi")" \
      "$(win "$root/packaging/windows/anvilcad.wxs")"
    rm -f "$dist"/*.wixpdb
  else
    echo "wix not found; skipping the .msi (dotnet tool install --global wix --version 5.0.2)" >&2
  fi
  ;;

*)
  sed -n '2,8p' "$0" >&2
  exit 2
  ;;
esac

ls -l "$dist"
