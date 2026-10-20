# Packaging and releases

- `package.sh macos|linux|windows [VERSION]` builds the release packages for one platform
  into `dist/`: a universal `.dmg` and `.tar.gz` on macOS, an `.msi` and `.zip` on Windows,
  and an AppImage and `.tar.gz` on Linux.
- `.github/workflows/release.yml` runs it on each platform. On a `v*` tag it attaches the
  packages and `SHA256SUMS.txt` to a GitHub Release. A manual run, or a pull request that
  changes this folder, builds them as a dry run.
- `update-package-managers.sh VERSION` writes the Homebrew cask and the winget manifest for
  a published release. It needs `gh` and `msiinfo` (msitools).
- `icons/` holds the app icon (`anvilcad.svg` is the source) and `macos/`, `linux/` and
  `windows/` hold the bundle metadata, `.desktop` file and AppRun, and WiX source.

## Making a release

1. Set `version` in `[workspace.package]` in the root `Cargo.toml`, merge, and push a tag
   with the same version: `git tag -a v0.2.0 -m "AnvilCAD 0.2.0" && git push origin v0.2.0`.
2. Wait for the Release workflow to publish the release.
3. Run `bash packaging/update-package-managers.sh 0.2.0` and commit what it writes.
4. Homebrew: copy `homebrew/anvilcad.rb` to `Casks/anvilcad.rb` in
   [swperb/homebrew-tap](https://github.com/swperb/homebrew-tap) and push. Users install
   it with `brew install --cask swperb/tap/anvilcad`.
5. winget: open a pull request on
   [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) adding the folder
   `winget/manifests/a/AnvilCAD/AnvilCAD/<version>/` at
   `manifests/a/AnvilCAD/AnvilCAD/<version>/`. Its bots validate the manifest and test
   the installer, and a moderator merges it.

The macOS app is signed with a Developer ID and notarized by `package.sh` when the signing
secrets are set (see `macos/sign.sh` and the header of `.github/workflows/release.yml`);
0.1.0 was the last unsigned release. The Windows and Linux packages are not code-signed.
The cask lives in a tap of our own because Homebrew's cask repository did not take the
unsigned 0.1.0 app.
