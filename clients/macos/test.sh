#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
LIB="$ROOT/target/aarch64-apple-darwin/release/libpasteleaf_core.a"
[[ -f "$LIB" ]] || LIB="$ROOT/target/release/libpasteleaf_core.a"
[[ -f "$LIB" ]] || { echo 'Build pasteleaf-core in release mode before running native ABI tests.' >&2; exit 1; }
mkdir -p "$ROOT/target/macos"
xcrun swiftc -swift-version 5 -target arm64-apple-macos14.0 \
  clients/macos/Sources/DesignTokens.generated.swift clients/macos/Sources/LifecyclePolicy.swift clients/macos/Sources/NativeAppearancePolicy.swift clients/macos/Sources/PanelLayout.swift clients/macos/Sources/SettingsWindowLayout.swift clients/macos/Sources/Models.swift clients/macos/Sources/HotkeyPolicy.swift clients/macos/Sources/AccessibilityPermission.swift clients/macos/Sources/ConflictPresentation.swift clients/macos/Sources/CoreBridge.swift clients/macos/Sources/AppModel.swift clients/macos/Sources/SafeImage.swift clients/macos/Tests/MacTests.swift \
  "$LIB" -framework AppKit -framework ImageIO -framework UniformTypeIdentifiers -framework Security -framework CoreFoundation -framework SystemConfiguration -lsqlite3 -lc++ -lz \
  -o "$ROOT/target/macos/PasteLeafTests"
PASTELEAF_DISABLE_SYNC_WORKER=1 PASTELEAF_DISABLE_KEYCHAIN=1 "$ROOT/target/macos/PasteLeafTests"
