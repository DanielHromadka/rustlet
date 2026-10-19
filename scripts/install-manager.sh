#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
case "${1:-}" in ''|--user) ;; *) echo 'Použití: ./scripts/install-manager.sh [--user]' >&2; exit 2 ;; esac
./scripts/build-manager.sh
./scripts/install-bundle.sh "$PWD/dist/Rustlet.app" cz.attosoft.tools-manager "${1:-}"
