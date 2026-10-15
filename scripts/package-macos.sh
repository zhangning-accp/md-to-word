#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
target="${1:-aarch64-apple-darwin}"
case "$target" in
  aarch64-apple-darwin) minimum_version=11.0 ;;
  x86_64-apple-darwin) minimum_version=10.14 ;;
  *) echo "Unsupported target: $target" >&2; exit 1 ;;
esac
MACOSX_DEPLOYMENT_TARGET="$minimum_version"
export MACOSX_DEPLOYMENT_TARGET
cargo build --release --locked --target "$target"
app="dist/$target/墨转 Word.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$target/release/md-to-word" "$app/Contents/MacOS/md-to-word"
cp assets/Info.plist "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :LSMinimumSystemVersion $minimum_version" "$app/Contents/Info.plist"
cp README.md LICENSE THIRD_PARTY_LICENSES.txt "$app/Contents/Resources/"
chmod +x "$app/Contents/MacOS/md-to-word"
# Ad-hoc signing provides a locally valid app bundle, not Developer ID notarization.
codesign --force --deep --sign - "$app"
ditto -c -k --sequesterRsrc --keepParent "$app" "dist/md-to-word-$target.zip"
echo "dist/md-to-word-$target.zip"
