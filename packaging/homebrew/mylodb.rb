# Homebrew cask for MyLoDB. It lives in a separate tap repository, farookhridoy/homebrew-mylodb, as Casks/mylodb.rb:
#   brew tap farookhridoy/mylodb && brew install --cask mylodb
# The version and the two checksums are updated for each release (see docs/RELEASING.md).
cask "mylodb" do
  arch arm: "aarch64", intel: "x64"

  version "0.6.0"
  sha256 arm:   "cf7bc0e7f102c4b3bf0c4fa7a89ab2ad25913ea2e8b7200e2c7856cd32e56c8c",
         intel: "df654eadff18d4217745901bb07a227d17afc31f1e9e60a20f7eeb2a662071d3"

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
