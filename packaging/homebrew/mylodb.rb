# Homebrew cask for MyLoDB. It lives in a separate tap repository, farookhridoy/homebrew-mylodb, as Casks/mylodb.rb:
#   brew tap farookhridoy/mylodb && brew install --cask mylodb
# The version and the two checksums are updated for each release (see docs/RELEASING.md).
cask "mylodb" do
  arch arm: "aarch64", intel: "x64"

  version "0.12.0"
  sha256 arm:   "cae8eddfc990b33ca7ab80bab1b6aa7876f2401159b3514e27a518d11560cc24",
         intel: "5db72d733f9075f5027ce139869a06878bc13ae2a7da2ae5ac4b5d639d5a2079"

  url "https://github.com/farookhridoy/mylodb/releases/download/v#{version}/MyLoDB_#{version}_#{arch}.dmg"
  name "MyLoDB"
  desc "MySQL, MariaDB and SQLite manager"
  homepage "https://farookhridoy.github.io/mylodb/"

  livecheck do
    url :url
    strategy :github_latest
  end

  # The app updates itself, so Homebrew should not replace it behind its back.
  auto_updates true

  app "MyLoDB.app"

  # The app is not notarised: remove the download flag so macOS opens it without the "could not verify" step.
  postflight do
    system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "#{appdir}/MyLoDB.app"]
  end

  zap trash: [
    "~/Library/Application Support/dev.mylodb.app",
    "~/Library/Caches/dev.mylodb.app",
    "~/Library/WebKit/dev.mylodb.app",
  ]
end
