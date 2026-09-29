#!/usr/bin/env bash
set -euo pipefail

release_dir="${1:?usage: rebuild-macos-dmg.sh <release-dir> <arch> [tauri-config-path]}"
arch="${2:?usage: rebuild-macos-dmg.sh <release-dir> <arch> [tauri-config-path]}"
tauri_config_path="${3:-apps/src-tauri/tauri.conf.json}"

bundle_root="${release_dir}/bundle"
test -d "$bundle_root" || {
  echo "macOS bundle root not found: $bundle_root"
  exit 1
}

product_name="$(
  python3 -c 'import json, pathlib, sys; print(json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")).get("productName") or "CodexManager")' "$tauri_config_path"
)"
bundle_dir="$(find "$bundle_root" -type d -path "*/${product_name}.app" | head -n 1)"
test -n "$bundle_dir" || {
  echo "macOS app bundle not found under: $bundle_root"
  exit 1
}

dmg_dir="${bundle_root}/dmg"
mkdir -p "$dmg_dir"
find "$dmg_dir" -type f -name '*.dmg' -delete

if [ "$arch" = "arm64" ]; then
  dmg_arch="aarch64"
else
  dmg_arch="x64"
fi

version="$(
  python3 - "$tauri_config_path" <<'PY'
import json
import pathlib
import sys

config_path = pathlib.Path(sys.argv[1])
payload = json.loads(config_path.read_text(encoding="utf-8"))
version = payload.get("version")
if not version:
    base_config_path = config_path.with_name("tauri.conf.json")
    if base_config_path != config_path and base_config_path.is_file():
        version = json.loads(base_config_path.read_text(encoding="utf-8")).get("version")
if not version:
    raise SystemExit(f"version missing from {config_path} and its base tauri.conf.json")
print(version)
PY
)"

stage_dir="$(mktemp -d)"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$stage_dir" "$temp_dir"' EXIT

ditto "$bundle_dir" "$stage_dir/${product_name}.app"
ln -s /Applications "$stage_dir/Applications"
helper_dir="assets/macos"
readme_name="README-macOS-first-launch.txt"
if [ "$product_name" = "CodexManagerLocal" ]; then
  helper_dir="assets/macos-local"
  readme_name="README-macOS-first-launch-local.txt"
fi
install -m 0755 "$helper_dir/Open ${product_name}.command" "$stage_dir/Open ${product_name}.command"
install -m 0644 "$helper_dir/$readme_name" "$stage_dir/$readme_name"

codesign --force --deep --sign - "$stage_dir/${product_name}.app"
codesign --verify --deep --strict "$stage_dir/${product_name}.app"

dmg_path="${dmg_dir}/${product_name}_${version}_${dmg_arch}.dmg"
temp_dmg_path="${temp_dir}/${product_name}_${version}_${dmg_arch}.dmg"
created=0

for attempt in 1 2 3; do
  rm -f "$temp_dmg_path"
  hdiutil detach "/Volumes/${product_name}" -force >/dev/null 2>&1 || true
  sync || true
  sleep "$attempt"
  if hdiutil create -volname "$product_name" -srcfolder "$stage_dir" -ov -format UDZO "$temp_dmg_path"; then
    created=1
    break
  fi
  echo "hdiutil create failed on attempt ${attempt}, retrying..."
done

if [ "$created" -ne 1 ]; then
  echo "macOS dmg create failed after retries"
  exit 1
fi

mv -f "$temp_dmg_path" "$dmg_path"
test -f "$dmg_path" || {
  echo "macOS dmg not created: $dmg_path"
  exit 1
}

echo "macOS dmg: $dmg_path"
