# Homebrew cask for MyLoDB. It lives in a separate tap repository, farookhridoy/homebrew-mylodb, as Casks/mylodb.rb:
#   brew tap farookhridoy/mylodb && brew install --cask mylodb
# The version and the two checksums are updated for each release (see docs/RELEASING.md).
cask "mylodb" do
  arch arm: "aarch64", intel: "x64"

  version "0.11.0"
  sha256 arm:   "57234b487ce6e616fc26a9db3f594a3f52b5dd197da0482034315c006eeafb2f",
         intel: "e5a931f10ea2b3671200707783f762f657821e4a3abe165fcae855a90cc4678d"

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
