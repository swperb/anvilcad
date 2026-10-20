# Homebrew cask for AnvilCAD, written by packaging/update-package-managers.sh.
#
#   brew install --cask swperb/tap/anvilcad
#
# Published in https://github.com/swperb/homebrew-tap. The app is signed with a Developer ID
# and notarized, so it opens without a Gatekeeper prompt.
cask "anvilcad" do
  version "0.1.1"
  sha256 "08b064e4ffc62842ee13d0254d2cfccc86fe2b60c711debca9e13d4c6da576b0"

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
