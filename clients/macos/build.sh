#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export MACOSX_DEPLOYMENT_TARGET=14.0
if [[ "${1:-}" != "--skip-core" ]]; then
  cargo build --locked -p pasteleaf-core --release --target aarch64-apple-darwin
fi
LIB="$ROOT/target/aarch64-apple-darwin/release/libpasteleaf_core.a"
[[ -f "$LIB" ]] || LIB="$ROOT/target/release/libpasteleaf_core.a"
[[ -f "$LIB" ]] || { echo "Missing release Rust core; build pasteleaf-core first." >&2; exit 1; }
DIST="${PASTELEAF_DIST_DIR:-$ROOT/dist}"
mkdir -p "$ROOT/target/macos" "$DIST"
DIST="$(cd "$DIST" && pwd -P)"
xcrun swiftc -swift-version 5 -O -whole-module-optimization -target arm64-apple-macos14.0 \
  clients/macos/Sources/*.swift "$LIB" \
  -framework AppKit -framework SwiftUI -framework Carbon -framework Security -framework ServiceManagement -framework ApplicationServices -framework CoreFoundation -framework SystemConfiguration \
  -lsqlite3 -lc++ -lz -o "$ROOT/target/macos/PasteLeaf"
xcrun swiftc -swift-version 5 -parse-as-library -target arm64-apple-macos14.0 clients/macos/Resources/GenerateIcon.swift -o "$ROOT/target/macos/generate-icon"
"$ROOT/target/macos/generate-icon" "$ROOT/assets/brand/pasteleaf-app-icon-v2-master.png" "$ROOT/target/macos/PasteLeaf.iconset"
iconutil -c icns "$ROOT/target/macos/PasteLeaf.iconset" -o "$ROOT/target/macos/PasteLeaf.icns"
for NAME in "PasteLeaf" "PasteLeaf Demo" "PasteLeaf Test"; do
  APP="$DIST/$NAME.app"
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
  cp "$ROOT/target/macos/PasteLeaf" "$APP/Contents/MacOS/PasteLeaf"
  cp "$ROOT/target/macos/PasteLeaf.icns" "$APP/Contents/Resources/PasteLeaf.icns"
  cp "$ROOT/clients/macos/Resources/Info.plist" "$APP/Contents/Info.plist"
  cp "$ROOT/clients/macos/Resources/demo-landscape.png" "$ROOT/clients/macos/Resources/Synthetic Notes.txt" "$ROOT/clients/macos/Resources/Synthetic Outline.md" "$APP/Contents/Resources/"
  if [[ "$NAME" == "PasteLeaf Demo" ]]; then
    /usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier io.pasteleaf.desktop.v2.demo' "$APP/Contents/Info.plist"
    /usr/libexec/PlistBuddy -c 'Set :CFBundleDisplayName PasteLeaf Demo' "$APP/Contents/Info.plist"
    /usr/libexec/PlistBuddy -c 'Add :PLDemo bool true' "$APP/Contents/Info.plist"
  elif [[ "$NAME" == "PasteLeaf Test" ]]; then
    /usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier io.pasteleaf.desktop.v2.test' "$APP/Contents/Info.plist"
    /usr/libexec/PlistBuddy -c 'Set :CFBundleDisplayName PasteLeaf Test' "$APP/Contents/Info.plist"
    /usr/libexec/PlistBuddy -c 'Add :PLTestLibrary bool true' "$APP/Contents/Info.plist"
  fi
  "${PASTELEAF_PYTHON:-python3}" "$ROOT/clients/macos/sign_app.py" "$APP"
done
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$DIST/PasteLeaf.app" "$DIST/PasteLeaf-macOS-arm64.zip"
echo "Built $DIST/PasteLeaf.app and isolated Demo/Test apps (signing mode reported above; not installed or notarized)."
