#!/bin/bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
app_path="/Applications/CodexManagerLocal.app"
if [ ! -d "$app_path" ]; then
  app_path="$script_dir/CodexManagerLocal.app"
fi

if [ ! -d "$app_path" ]; then
  echo "CodexManagerLocal.app was not found."
  exit 1
fi

xattr -dr com.apple.quarantine "$app_path" 2>/dev/null || true
open "$app_path"
