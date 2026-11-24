# Homebrew cask for AnvilCAD, written by packaging/update-package-managers.sh.
#
#   brew install --cask swperb/tap/anvilcad
#
# Published in https://github.com/swperb/homebrew-tap. The app is signed with a Developer ID
# and notarized, so it opens without a Gatekeeper prompt.
cask "anvilcad" do
  version "0.2.0"
  sha256 "7ad34e9009ef0f948197e497cc03ec24d5ceefaa38deddb3337fd55cb0740d28"

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
end
