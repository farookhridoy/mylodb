#!/bin/sh
# Installs the latest MyLoDB into /Applications without the "Apple could not verify" step.
#   curl -fsSL https://raw.githubusercontent.com/farookhridoy/mylodb/main/scripts/install.sh | sh
# It downloads the .dmg for this Mac (Apple Silicon or Intel) from the GitHub release, checks its SHA-256,
# copies the app, and removes the download quarantine flag. Read it first if you like: it is short.
# Options (environment): MYLODB_VERSION=0.4.4 (default: latest), MYLODB_DEST=/Applications.
set -eu

REPO="farookhridoy/mylodb"
DEST="${MYLODB_DEST:-/Applications}"

[ "$(uname -s)" = "Darwin" ] || { echo "MyLoDB is a macOS app." >&2; exit 1; }
case "$(uname -m)" in
  arm64) ARCH="aarch64" ;;
  x86_64) ARCH="x64" ;;
  *) echo "Unsupported processor: $(uname -m)" >&2; exit 1 ;;
esac

if [ -n "${MYLODB_VERSION:-}" ]; then
  VERSION="${MYLODB_VERSION#v}"
else
  VERSION="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)"
fi
[ -n "$VERSION" ] || { echo "Could not find the latest version. Check your connection." >&2; exit 1; }

FILE="MyLoDB_${VERSION}_${ARCH}.dmg"
URL="https://github.com/$REPO/releases/download/v$VERSION/$FILE"
TMP="$(mktemp -d)"
MNT="$TMP/mnt"
cleanup() { hdiutil detach "$MNT" -quiet >/dev/null 2>&1 || true; rm -rf "$TMP"; }
trap cleanup EXIT INT TERM

echo "Downloading MyLoDB $VERSION ($ARCH)…"
curl -fL --progress-bar -o "$TMP/$FILE" "$URL"
curl -fsSL -o "$TMP/$FILE.sha256" "$URL.sha256"

echo "Checking the download…"
WANT="$(awk '{print $1}' "$TMP/$FILE.sha256")"
GOT="$(shasum -a 256 "$TMP/$FILE" | awk '{print $1}')"
[ -n "$WANT" ] && [ "$WANT" = "$GOT" ] || { echo "The checksum does not match. Nothing was installed." >&2; exit 1; }

mkdir -p "$MNT"
hdiutil attach "$TMP/$FILE" -nobrowse -readonly -quiet -mountpoint "$MNT"
[ -d "$MNT/MyLoDB.app" ] || { echo "MyLoDB.app is not inside the disk image." >&2; exit 1; }

mkdir -p "$DEST"
if [ -d "$DEST/MyLoDB.app" ]; then
  echo "Replacing the MyLoDB in $DEST. Your connections and settings are kept (they are not inside the app)."
  rm -rf "$DEST/MyLoDB.app"
fi
cp -R "$MNT/MyLoDB.app" "$DEST/MyLoDB.app"
xattr -dr com.apple.quarantine "$DEST/MyLoDB.app" 2>/dev/null || true

echo "Installed MyLoDB $VERSION in $DEST. Open it from Launchpad or Spotlight."
