#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if ! command -v cargo >/dev/null 2>&1; then export PATH="$HOME/.cargo/bin:$PATH"; fi
cargo build --locked --release -p rustlet
app="$PWD/dist/Rustlet.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/rustlet "$app/Contents/MacOS/"
cp resources/Info.plist "$app/Contents/Info.plist"
cp resources/catalog.json "$app/Contents/Resources/"
identity="${CODE_SIGN_IDENTITY:-}"
if [[ -z "$identity" ]]; then identity=$(/usr/bin/security find-identity -v -p codesigning | awk '/Developer ID Application:/ {print $2; exit}'); fi
if [[ -z "$identity" ]]; then identity=$(/usr/bin/security find-identity -v -p codesigning | awk '/Apple Development:/ {print $2; exit}'); fi
identity="${identity:--}"
codesign --force --sign "$identity" --timestamp=none --options runtime --identifier cz.attosoft.tools-manager "$app"
codesign --verify --deep --strict "$app"
echo "Vytvořeno: $app"
