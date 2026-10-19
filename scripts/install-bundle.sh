#!/bin/bash
# Shared local installer for the tool and its manager.
set -euo pipefail
source_app=$1
expected_id=$2
case "${3:-}" in
  '') if [[ -w /Applications ]]; then install_root=/Applications; else install_root="$HOME/Applications"; fi ;;
  --user) install_root="$HOME/Applications" ;;
  *) echo 'Použití: install-bundle.sh zdroj.app bundle.id [--user]' >&2; exit 2 ;;
esac
bundle_id=$(/usr/bin/plutil -extract CFBundleIdentifier raw "$source_app/Contents/Info.plist")
if [[ "$bundle_id" != "$expected_id" ]]; then echo 'Zdroj má neočekávaný identifikátor.' >&2; exit 1; fi
name=$(basename "$source_app")
executable=$(/usr/bin/plutil -extract CFBundleExecutable raw "$source_app/Contents/Info.plist")
mkdir -p "$install_root"
target_app="$install_root/$name"
if [[ -e "$target_app" ]]; then
  bundle_id=$(/usr/bin/plutil -extract CFBundleIdentifier raw "$target_app/Contents/Info.plist")
  if [[ "$bundle_id" != "$expected_id" ]]; then echo "Cíl patří jiné aplikaci: $target_app" >&2; exit 1; fi
fi
while read -r app_pid app_executable; do
  if [[ "$app_executable" == "$target_app/Contents/MacOS/$executable" || "$app_executable" == "$source_app/Contents/MacOS/$executable" ]]; then
    kill -TERM "$app_pid"
    for ((attempt=0; attempt<40; attempt++)); do
      if ! kill -0 "$app_pid" 2>/dev/null; then break; fi
      sleep 0.05
    done
    if kill -0 "$app_pid" 2>/dev/null; then echo "Nejdřív ukonči $name." >&2; exit 1; fi
  fi
done < <(/bin/ps -axo pid=,comm=)
install_stage=$(mktemp -d "$install_root/.attosoft-install.XXXXXX")
trap 'rm -rf "$install_stage"' EXIT
/usr/bin/ditto "$source_app" "$install_stage/$name"
/usr/bin/codesign --verify --deep --strict "$install_stage/$name"
if [[ -e "$target_app" ]]; then mv "$target_app" "$install_stage/previous.app"; fi
if ! mv "$install_stage/$name" "$target_app"; then
  if [[ -e "$install_stage/previous.app" ]]; then mv "$install_stage/previous.app" "$target_app"; fi
  exit 1
fi
/usr/bin/open "$target_app" --args --settings
echo "Nainstalováno a spuštěno: $target_app"
