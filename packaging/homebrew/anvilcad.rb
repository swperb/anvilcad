# Homebrew cask for AnvilCAD, written by packaging/update-package-managers.sh.
#
#   brew install --cask swperb/tap/anvilcad
#
# Published in https://github.com/swperb/homebrew-tap rather than homebrew/cask, because
# the app is not signed with an Apple Developer ID and homebrew/cask no longer takes apps
# that fail Gatekeeper.
cask "anvilcad" do
  version "0.1.0"
  sha256 "ddebbf86b67dc947c8ac14f090ad2e2aad719346507af0b1d2bc8ebed882d507"

  url "https://github.com/swperb/anvilcad/releases/download/v#{version}/AnvilCAD-#{version}-macos-universal.dmg"
  name "AnvilCAD"
  desc "Free and open-source parametric 3D CAD"
  homepage "https://github.com/swperb/anvilcad"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on :macos

  app "AnvilCAD.app"
  binary "#{appdir}/AnvilCAD.app/Contents/MacOS/anvilcad-agent"
  binary "#{appdir}/AnvilCAD.app/Contents/MacOS/anvil-bulk"

  zap trash: "~/.config/anvilcad"

  caveats <<~EOS
    AnvilCAD is not signed with an Apple Developer ID yet. The first time you open it,
    macOS says it cannot verify the developer: click Done, then open System Settings >
    Privacy & Security and click Open Anyway next to AnvilCAD.

    Until then macOS also stops anvilcad-agent and anvil-bulk in a terminal. To trust
    the app and its tools in one step:
      xattr -dr com.apple.quarantine #{appdir}/AnvilCAD.app
  EOS
end
