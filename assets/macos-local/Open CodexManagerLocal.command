#!/bin/bash
set -euo pipefail

SELF_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_IN_APPLICATIONS="/Applications/CodexManagerLocal.app"
APP_NEXT_TO_SCRIPT="${SELF_DIR}/CodexManagerLocal.app"

TARGET_APP=""
if [ -d "$APP_IN_APPLICATIONS" ]; then
  TARGET_APP="$APP_IN_APPLICATIONS"
elif [ -d "$APP_NEXT_TO_SCRIPT" ]; then
  TARGET_APP="$APP_NEXT_TO_SCRIPT"
fi

if [ -z "$TARGET_APP" ]; then
  echo "CodexManagerLocal.app not found."
  echo
  echo "1. Drag CodexManagerLocal.app into Applications first."
  echo "2. Then run this script again."
  echo
  read -r -p "Press Enter to exit..."
  exit 1
fi

echo "Removing quarantine from:"
echo "  $TARGET_APP"
echo
xattr -dr com.apple.quarantine "$TARGET_APP" || true

echo "Opening CodexManagerLocal..."
open "$TARGET_APP"
echo
echo "If macOS still blocks the app, right-click CodexManagerLocal.app and choose Open once."
echo
read -r -p "Press Enter to exit..."
