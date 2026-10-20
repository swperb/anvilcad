# Code signing and notarization for the macOS packages. Sourced by packaging/package.sh.
#
# Both steps are optional and controlled by the environment:
#
#   ANVIL_MACOS_SIGN_IDENTITY  A "Developer ID Application" identity in a keychain, by name
#                              ("Developer ID Application: Name (TEAMID)") or SHA-1 hash.
#                              Unset: everything gets an ad-hoc signature, as before, which
#                              runs on Apple Silicon but which Gatekeeper rejects on download.
#   ANVIL_NOTARY_PROFILE       A notarytool keychain profile, made once with
#                              `xcrun notarytool store-credentials <name> ...`. Needs the
#                              identity above. Unset: the packages are signed but not
#                              notarized.
#   ANVIL_KEYCHAIN             Optional keychain holding the identity and the profile (the
#                              release workflow uses a temporary one). Defaults to the search
#                              list.
#
# Signing uses the hardened runtime (required for notarization) with no entitlements.
# AnvilCAD needs none: it is plain native code with no JIT (Lua 5.4 is the interpreter, not
# LuaJIT), draws through Metal, loads no plug-ins or other libraries at run time, and
# reaches files through the standard open and save panels.

macos_identity="${ANVIL_MACOS_SIGN_IDENTITY:-}"
macos_profile="${ANVIL_NOTARY_PROFILE:-}"
macos_keychain=()
if [ -n "${ANVIL_KEYCHAIN:-}" ]; then
  macos_keychain=(--keychain "$ANVIL_KEYCHAIN")
fi

if [ -n "$macos_profile" ] && [ -z "$macos_identity" ]; then
  echo "ANVIL_NOTARY_PROFILE is set but ANVIL_MACOS_SIGN_IDENTITY is not; Apple only" \
    "notarizes Developer ID signed code, so nothing will be notarized" >&2
  macos_profile=""
fi

# macos_sign PATH [IDENTIFIER]: sign one binary, bundle or disk image.
macos_sign() {
  local id=()
  if [ -n "${2:-}" ]; then id=(--identifier "$2"); fi
  if [ -z "$macos_identity" ]; then
    codesign --force --sign - ${id[@]+"${id[@]}"} "$1"
  else
    codesign --force --sign "$macos_identity" ${macos_keychain[@]+"${macos_keychain[@]}"} --options runtime \
      --timestamp ${id[@]+"${id[@]}"} "$1"
  fi
}

# macos_notarize PATH: submit a .zip or .dmg and wait for Apple's verdict. On rejection the
# log says why, and the build stops.
macos_notarize() {
  local out id status
  out="$(xcrun notarytool submit "$1" --keychain-profile "$macos_profile" \
    ${macos_keychain[@]+"${macos_keychain[@]}"} --wait --timeout 1h --output-format plist)" || true
  id="$(plutil -extract id raw -o - - <<<"$out" 2>/dev/null || true)"
  status="$(plutil -extract status raw -o - - <<<"$out" 2>/dev/null || true)"
  echo "notarization of $(basename "$1"): ${status:-no answer} (submission ${id:-none})"
  if [ "$status" != "Accepted" ]; then
    echo "$out" >&2
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" --keychain-profile "$macos_profile" ${macos_keychain[@]+"${macos_keychain[@]}"} >&2 || true
    fi
    return 1
  fi
}
