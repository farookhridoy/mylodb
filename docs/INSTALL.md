# Installing MyLoDB

MyLoDB is not signed with a paid Apple certificate (that costs $99 a year), so macOS shows "Apple could not verify MyLoDB" the first time you open a downloaded copy. The app is open source and every release has a SHA-256 checksum. There are four ways to install it; the first two skip the warning.

## 1. One command (Terminal)

```sh
curl -fsSL https://raw.githubusercontent.com/farookhridoy/mylodb/main/scripts/install.sh | sh
```

It picks the right file for your Mac (Apple Silicon or Intel), checks its checksum, copies **MyLoDB** into **Applications** and removes the download flag, so it opens normally. Run it again to update; your connections and settings are kept. Read the script first if you like: it is [scripts/install.sh](../scripts/install.sh) and is about 50 lines. Set `MYLODB_VERSION=0.4.4` to install a specific version.

## 2. Homebrew

```sh
brew tap farookhridoy/mylodb
brew trust farookhridoy/mylodb
brew install --cask mylodb
```

Newer Homebrew refuses casks from a third-party tap until you trust it (`brew trust`). The tap is [farookhridoy/homebrew-mylodb](https://github.com/farookhridoy/homebrew-mylodb); read its one file first. Not yet tested end to end: please report any error.

The cask is also kept in [packaging/homebrew/mylodb.rb](../packaging/homebrew/mylodb.rb). The app updates itself, so you do not need `brew upgrade`.

## 3. The .dmg, by hand

1. Download the `.dmg` for your Mac from the [Releases page](https://github.com/farookhridoy/mylodb/releases/latest): `aarch64` for Apple Silicon (M1 and later), `x64` for Intel. Not sure? Apple menu, then **About This Mac**.
2. Open it and drag **MyLoDB** into **Applications**.
3. Open MyLoDB. When macOS says it could not verify the app, click **Done** (not *Move to Trash*).
4. Open **System Settings → Privacy & Security**, scroll to **Security**, click **Open Anyway** next to MyLoDB and enter your password.

You do this once. Or, in Terminal: `xattr -dr com.apple.quarantine /Applications/MyLoDB.app`.

## 4. Check the download

Each `.dmg` has a `.sha256` file next to it:

```sh
shasum -a 256 MyLoDB_0.4.4_aarch64.dmg
```

The first column must equal the text in the `.sha256` file.

## About the Keychain prompt

Because the app is not signed with a stable certificate, macOS asks for your login password again for each saved connection after an update. Choose **Always Allow** to stop it for that build. To avoid it without a paid certificate, open **Preferences → Security → Saved passwords** and move your passwords to an encrypted file: you type one master password per session instead.

## Updates

From 0.4.0 the app offers new versions by itself (**Help → Check for Updates…**). Updates are signed with their own key and checked before they are installed, so they do not need the steps above again.
