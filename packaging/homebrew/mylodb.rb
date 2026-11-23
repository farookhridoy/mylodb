# Homebrew cask for MyLoDB. It lives in a separate tap repository, farookhridoy/homebrew-mylodb, as Casks/mylodb.rb:
#   brew tap farookhridoy/mylodb && brew install --cask mylodb
# The version and the two checksums are updated for each release (see docs/RELEASING.md).
cask "mylodb" do
  arch arm: "aarch64", intel: "x64"

  version "0.4.4"
  sha256 arm:   "f3d1bfaed90579e47cbb344bfd49d4531f349dec4949b71759f86c33b1c87232",
         intel: "615d8922b80aa4decbbdf37d1a197a5a7beb68ec1a40067374481877a43d4bec"

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
