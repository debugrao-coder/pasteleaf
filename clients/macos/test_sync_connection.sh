#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
LIB="$ROOT/target/aarch64-apple-darwin/release/libpasteleaf_core.a"
[[ -f "$LIB" ]] || LIB="$ROOT/target/release/libpasteleaf_core.a"
mkdir -p "$ROOT/target/macos"
xcrun swiftc -swift-version 5 -warnings-as-errors -target arm64-apple-macos14.0 \
  clients/macos/Sources/DesignTokens.generated.swift clients/macos/Sources/Models.swift \
  clients/macos/Sources/CoreBridge.swift clients/macos/Sources/SyncConnectionDraft.swift \
  clients/macos/Tests/SyncConnectionTests.swift "$LIB" \
  -framework AppKit -framework Security -framework CoreFoundation -framework SystemConfiguration \
  -lsqlite3 -lc++ -lz -o "$ROOT/target/macos/SyncConnectionTests"
PASTELEAF_DISABLE_SYNC_WORKER=1 PASTELEAF_DISABLE_KEYCHAIN=1 "$ROOT/target/macos/SyncConnectionTests"
