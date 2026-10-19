#!/bin/bash
# Create macOS installer artifacts without installing or launching the app.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
app_name="Rustlet"
slug="rustlet"
./scripts/build-manager.sh
app="$PWD/dist/$app_name.app"
version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")
bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")
architecture=$(uname -m)
codesign --verify --deep --strict "$app"
lipo -verify_arch "$architecture" "$app/Contents/MacOS/rustlet"
if [[ -n "${GITHUB_REF_TYPE:-}" && "$GITHUB_REF_TYPE" == tag && "$GITHUB_REF_NAME" != "v$version" ]]; then
  echo "Tag $GITHUB_REF_NAME does not match app version v$version" >&2
  exit 1
fi
out="$PWD/dist/installers"
mkdir -p "$out"
name="$slug-$version-macos-$architecture"
pkgbuild --component "$app" --install-location /Applications \
  --identifier "$bundle_id.installer" --version "$version" "$out/$name.pkg"
# A drag-to-Applications alternative alongside the native Installer package.
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
ditto "$app" "$stage/$app_name.app"
ln -s /Applications "$stage/Applications"
hdiutil create -volname "$app_name" -srcfolder "$stage" -ov -format UDZO "$out/$name.dmg"
hdiutil verify "$out/$name.dmg"
pkgutil --expand "$out/$name.pkg" "$stage/expanded-package"
# Deliberately do not install the package in CI or start camera/accessibility tools.
(cd "$out" && shasum -a 256 "$name.pkg" "$name.dmg" > "$name.sha256")
echo "Installers: $out/$name.{pkg,dmg}"
