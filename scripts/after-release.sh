#!/usr/bin/env bash
# Run once after a release is published: brings the Homebrew tap and the in-repo cask copy to that release.
#   scripts/after-release.sh [tag]      (default: the latest release)
# Needs the GitHub CLI (gh) signed in with write access to farookhridoy/homebrew-mylodb.
set -euo pipefail
repo=farookhridoy/mylodb
tap=farookhridoy/homebrew-mylodb
tag=${1:-$(gh release view --repo "$repo" --json tagName -q .tagName)}
v=${tag#v}
sum() { gh release download "$tag" --repo "$repo" --pattern "MyLoDB_${v}_$1.dmg.sha256" -O - | cut -d' ' -f1; }
arm=$(sum aarch64); intel=$(sum x64)
[ ${#arm} = 64 ] && [ ${#intel} = 64 ] || { echo "could not read both checksums for $tag" >&2; exit 1; }

cask=packaging/homebrew/mylodb.rb
python3 - "$cask" "$v" "$arm" "$intel" <<'PY'
import re, sys
p, v, arm, intel = sys.argv[1:]
s = open(p).read()
s = re.sub(r'version "[^"]+"', f'version "{v}"', s, 1)
s = re.sub(r'(sha256 arm:\s+)"[0-9a-f]{64}"', rf'\1"{arm}"', s, 1)
s = re.sub(r'(intel:\s+)"[0-9a-f]{64}"', rf'\1"{intel}"', s, 1)
open(p, "w").write(s)
PY

# push the same file to the tap
sha=$(gh api "repos/$tap/contents/Casks/mylodb.rb" -q .sha)
gh api --method PUT "repos/$tap/contents/Casks/mylodb.rb" -f message="mylodb $v" -f sha="$sha" -f content="$(base64 < "$cask" | tr -d '\n')" > /dev/null
echo "tap updated to $v; $cask changed in the working tree (commit it)"
