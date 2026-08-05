#!/usr/bin/env bash
set -euo pipefail

APP="/Applications/CodexManagerLocal.app"
if [ ! -d "$APP" ]; then
  DIR="$(cd "$(dirname "$0")" && pwd)"
  APP="$DIR/CodexManagerLocal.app"
fi

if [ ! -d "$APP" ]; then
  echo "CodexManagerLocal.app not found."
  exit 1
fi

xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
open "$APP"
