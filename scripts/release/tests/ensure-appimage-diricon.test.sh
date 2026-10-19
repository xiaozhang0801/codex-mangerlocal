#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repair_script="$script_dir/../ensure-appimage-diricon.sh"
for tool in unsquashfs mksquashfs; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done

test_dir="$(mktemp -d)"
trap 'rm -rf -- "$test_dir"' EXIT
offset=512
passed=0

fail() { echo "FAIL: $*" >&2; exit 1; }

new_fixture() {
  case_dir="$test_dir/$1"
  app_dir="$case_dir/build/AppDir"
  image="$case_dir/fixture.AppImage"
  fixture_compression=gzip
  runtime_extract_fails=false
  runtime_private_directories=false
  mkdir -p "$app_dir/usr/share/icons" "$app_dir/usr/share/applications"
  # The full one-pixel PNG keeps the fixture meaningful without image tools.
  printf '%s' 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aNF8AAAAASUVORK5CYII=' | base64 -d > "$app_dir/usr/share/icons/App.png"
  ln -s usr/share/icons/App.png "$app_dir/App.png"
  printf '[Desktop Entry]\nType=Application\nName=Fixture\nExec=fixture\nIcon=App\n' > "$app_dir/usr/share/applications/App.desktop"
  ln -s usr/share/applications/App.desktop "$app_dir/App.desktop"
  mkdir -p "$app_dir/usr/bin" "$app_dir/usr/share/resources"
  printf 'fixture-data\n' > "$app_dir/usr/share/resources/payload"
  cat > "$app_dir/AppRun" <<'SH'
#!/usr/bin/env bash
exec "$(dirname -- "$0")/AppRun.wrapped"
SH
  cat > "$app_dir/AppRun.wrapped" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
[[ "$(cat "$app_dir/usr/share/resources/payload")" == fixture-data ]]
exec "$app_dir/usr/bin/helper"
SH
  printf '#!/usr/bin/env bash\necho fixture-launch-ok\n' > "$app_dir/usr/bin/helper"
  chmod 755 "$app_dir/AppRun" "$app_dir/AppRun.wrapped" "$app_dir/usr/bin/helper"
}

pack_fixture() {
  # This fixture implements the offset/extraction interface with unsquashfs;
  # real embedded AppImage runtime compatibility needs an actual release image.
  local extract_command
  extract_command='exec unsquashfs -quiet -offset 512 -dest squashfs-root "$(readlink -f -- "$0")"'
  if [[ "$runtime_private_directories" == true ]]; then
    extract_command='unsquashfs -quiet -offset 512 -dest squashfs-root "$(readlink -f -- "$0")" && find squashfs-root -type d -exec chmod 700 {} +'
  fi
  if [[ "$runtime_extract_fails" == true ]]; then extract_command='exit 73'; fi
  printf '#!/usr/bin/env bash\ncase "${1:-}" in\n--appimage-offset) echo %s;;\n--appimage-extract) %s;;\n*) exit 1;;\nesac\nexit 0\n' "$offset" "$extract_command" > "$case_dir/runtime"
  truncate -s "$offset" "$case_dir/runtime"
  mksquashfs "$app_dir" "$case_dir/filesystem" -noappend -comp "$fixture_compression" -quiet -processors 1 -all-root "$@" >/dev/null
  cat "$case_dir/runtime" "$case_dir/filesystem" > "$image"
  chmod 751 "$image"
  before_hash="$(sha256sum "$image" | cut -d ' ' -f1)"
}

repair_and_check() {
  local changed="$1" actual_mode
  bash "$repair_script" "$image" > "$case_dir/repair.log" 2>&1 || {
    cat "$case_dir/repair.log" >&2
    fail "repair failed for ${case_dir##*/}"
  }
  actual_mode="$(stat -c %a "$image")"
  [[ "$actual_mode" == 751 ]] || fail 'input mode changed'
  head -c "$offset" "$image" > "$case_dir/repaired-runtime"
  cmp "$case_dir/runtime" "$case_dir/repaired-runtime" || fail 'runtime changed'
  [[ "$("$image" --appimage-offset)" == "$offset" ]] || fail 'offset changed'
  local expected_compression="$fixture_compression" actual_compression
  if [[ "$changed" == true ]]; then expected_compression=gzip; fi
  actual_compression="$(LC_ALL=C unsquashfs -s -offset "$offset" "$image" | sed -n 's/^Compression[[:space:]]*//p')"
  [[ "$actual_compression" == "$expected_compression" ]] || fail "unexpected repaired compression: $actual_compression (expected $expected_compression)"
  if [[ "$changed" == false ]]; then
    [[ "$(sha256sum "$image" | cut -d ' ' -f1)" == "$before_hash" ]] || fail 'valid image was repacked'
  fi
  local repaired_hash
  repaired_hash="$(sha256sum "$image" | cut -d ' ' -f1)"
  bash "$repair_script" "$image" >> "$case_dir/repair.log" 2>&1 || fail 'second verification failed'
  [[ "$(sha256sum "$image" | cut -d ' ' -f1)" == "$repaired_hash" ]] || fail 'repair is not idempotent'
  # Removing the build directory reproduces the catalog's isolated environment.
  mv "$case_dir/build" "$case_dir/build-hidden"
  unsquashfs -quiet -offset "$offset" -dest "$case_dir/check" "$image" >/dev/null
  [[ -f "$case_dir/check/.DirIcon" ]] || fail 'icon is unavailable outside build tree'
  cmp "$case_dir/check/.DirIcon" "$case_dir/build-hidden/AppDir/usr/share/icons/App.png" || fail 'icon contents changed'
  [[ -f "$case_dir/check/App.desktop" ]] || fail 'desktop entry is unavailable outside build tree'
  [[ -z "$(find "$case_dir/check" -type d ! -perm -0555 -print -quit)" ]] || fail 'directory is not readable/traversable by all users'
  [[ -z "$(find "$case_dir/check" -type f \( ! -perm -0444 -o \( -perm /0111 ! -perm -0111 \) \) -print -quit)" ]] || fail 'resource or executable permissions are not portable'
  [[ -z "$(find -L "$case_dir/check/AppRun" "$case_dir/check/AppRun.wrapped" ! -perm -0111 -print -quit)" ]] || fail 'launcher is not executable by all users'
  if [[ "$changed" == true ]]; then
    [[ "$(readlink "$case_dir/check/.DirIcon")" != /* ]] || fail 'absolute icon link remains'
  fi
  passed=$((passed + 1))
  echo "PASS: ${case_dir##*/}"
}

run_as_unprivileged_user() {
  local output
  if [[ "$EUID" == 0 ]]; then
    command -v setpriv >/dev/null || fail 'root test requires setpriv for the nonroot launch check'
    chmod a+rx "$test_dir" "$case_dir"
    output="$(setpriv --reuid=65534 --regid=65534 --clear-groups "$case_dir/check/AppRun")" || fail 'nonroot user could not start launcher'
  else
    output="$("$case_dir/check/AppRun")" || fail 'nonroot user could not start launcher'
  fi
  [[ "$output" == fixture-launch-ok ]] || fail 'nonroot launcher did not execute helper/read resources'
}

new_fixture valid-relative
ln -s App.png "$app_dir/.DirIcon"
pack_fixture
repair_and_check false

new_fixture valid-regular
cp "$app_dir/usr/share/icons/App.png" "$app_dir/.DirIcon"
pack_fixture
repair_and_check false

new_fixture compatible-zstd
fixture_compression=zstd
ln -s App.png "$app_dir/.DirIcon"
pack_fixture
repair_and_check false

new_fixture incompatible-xz
fixture_compression=xz
ln -s App.png "$app_dir/.DirIcon"
pack_fixture
repair_and_check true

new_fixture runtime-private-directories
runtime_private_directories=true
ln -s App.png "$app_dir/.DirIcon"
pack_fixture
repair_and_check false

new_fixture missing
pack_fixture
repair_and_check true

new_fixture dangling
ln -s missing.png "$app_dir/.DirIcon"
pack_fixture
repair_and_check true

new_fixture absolute-existing
ln -s "$app_dir/App.png" "$app_dir/.DirIcon"
[[ -e "$app_dir/.DirIcon" ]] || fail 'absolute-link regression needs the build target to exist'
pack_fixture
repair_and_check true

new_fixture outside-relative
cp "$app_dir/usr/share/icons/App.png" "$case_dir/build/external.png"
ln -s ../external.png "$app_dir/.DirIcon"
[[ -e "$app_dir/.DirIcon" ]] || fail 'outside-link fixture target must exist'
pack_fixture
repair_and_check true

new_fixture absolute-intermediate
mkdir "$app_dir/portable"
cp "$app_dir/usr/share/icons/App.png" "$app_dir/portable/App.png"
ln -s portable/App.png "$app_dir/Portable.png"
ln -s "$app_dir/usr/share/icons" "$app_dir/build-icons"
rm "$app_dir/App.png"
ln -s build-icons/App.png "$app_dir/App.png"
ln -s App.png "$app_dir/.DirIcon"
[[ -e "$app_dir/.DirIcon" ]] || fail 'intermediate-link regression needs its target to exist'
pack_fixture
repair_and_check true
[[ "$(readlink "$case_dir/check/.DirIcon")" == Portable.png ]] || fail 'nonportable root PNG was selected'

new_fixture absolute-desktop
ln -s App.png "$app_dir/.DirIcon"
rm "$app_dir/App.desktop"
ln -s "$app_dir/usr/share/applications/App.desktop" "$app_dir/App.desktop"
pack_fixture
repair_and_check true
[[ "$(readlink "$case_dir/check/App.desktop")" == usr/share/applications/App.desktop ]] || fail 'desktop link was not normalized'

new_fixture no-portable-icon
rm "$app_dir/App.png"
ln -s "$app_dir/usr/share/icons/App.png" "$app_dir/App.png"
ln -s App.png "$app_dir/.DirIcon"
pack_fixture
if bash "$repair_script" "$image" > "$case_dir/repair.log" 2>&1; then
  fail 'accepted an image whose only root PNG relies on the build directory'
fi
[[ "$(sha256sum "$image" | cut -d ' ' -f1)" == "$before_hash" ]] || fail 'failed repair changed the input'
[[ "$(stat -c %a "$image")" == 751 ]] || fail 'failed repair changed input mode'
passed=$((passed + 1))
echo "PASS: no-portable-icon (input preserved)"

new_fixture verification-failure
pack_fixture
real_unsquashfs="$(command -v unsquashfs)"
mkdir "$case_dir/bin"
cat > "$case_dir/bin/unsquashfs" <<'SH'
#!/usr/bin/env bash
for arg in "$@"; do
  if [[ "$arg" == */verify ]]; then exit 97; fi
done
exec "$REAL_UNSQUASHFS" "$@"
SH
chmod +x "$case_dir/bin/unsquashfs"
if PATH="$case_dir/bin:$PATH" REAL_UNSQUASHFS="$real_unsquashfs" bash "$repair_script" "$image" > "$case_dir/repair.log" 2>&1; then
  fail 'accepted a failed independent extraction'
fi
[[ "$(sha256sum "$image" | cut -d ' ' -f1)" == "$before_hash" ]] || fail 'verification failure replaced the input'
[[ "$(stat -c %a "$image")" == 751 ]] || fail 'verification failure changed input mode'
[[ -z "$(find "$case_dir" -maxdepth 1 -name '.appimage-diricon.*' -print -quit)" ]] || fail 'temporary replacement was not cleaned up'
passed=$((passed + 1))
echo "PASS: verification-failure (input preserved)"

new_fixture restricted-permissions
ln -s App.png "$app_dir/.DirIcon"
chmod 770 "$app_dir/AppRun" "$app_dir/AppRun.wrapped"
chmod 700 "$app_dir/usr/bin/helper" "$app_dir/usr/share/resources"
# Keep source data readable by an ordinary CI user; only the packaged inode is
# unreadable. Otherwise mksquashfs can replace unreadable source data with an
# empty file without failing, so the nonroot launch tests the wrong failure.
pack_fixture -p 'usr/share/resources/payload m 000 0 0'
unsquashfs -quiet -offset "$offset" -dest "$case_dir/before-repair" "$image" >/dev/null
[[ "$(stat -c %a "$case_dir/before-repair/usr/share/resources/payload")" == 0 ]] || fail 'restricted resource was not packed with mode 000'
repair_and_check true
if [[ "$EUID" == 0 ]]; then
  [[ "$(stat -c %u:%g "$case_dir/check/AppRun.wrapped")" == 0:0 ]] || fail 'root-owned regression fixture lost its owner'
fi
run_as_unprivileged_user
echo 'PASS: root-owned launchers and restricted resources run as a nonroot user'

new_fixture non-executable-launchers
ln -s App.png "$app_dir/.DirIcon"
chmod 644 "$app_dir/AppRun" "$app_dir/AppRun.wrapped"
pack_fixture
repair_and_check true
run_as_unprivileged_user

new_fixture relative-launcher
ln -s App.png "$app_dir/.DirIcon"
mv "$app_dir/AppRun" "$app_dir/usr/bin/launcher"
ln -s usr/bin/launcher "$app_dir/AppRun"
chmod 644 "$app_dir/usr/bin/launcher"
pack_fixture
repair_and_check true
run_as_unprivileged_user

new_fixture cyclic-icon
ln -s icon-cycle "$app_dir/.DirIcon"
ln -s .DirIcon "$app_dir/icon-cycle"
pack_fixture
repair_and_check true

new_fixture external-launcher
ln -s App.png "$app_dir/.DirIcon"
mv "$app_dir/AppRun" "$case_dir/external-launcher"
chmod 700 "$case_dir/external-launcher"
ln -s "$case_dir/external-launcher" "$app_dir/AppRun"
pack_fixture
if bash "$repair_script" "$image" > "$case_dir/repair.log" 2>&1; then
  fail 'accepted an absolute launcher link'
fi
[[ "$(stat -c %a "$case_dir/external-launcher")" == 700 ]] || fail 'permission repair followed an external launcher link'
[[ "$(sha256sum "$image" | cut -d ' ' -f1)" == "$before_hash" ]] || fail 'unsafe launcher failure changed the input'
passed=$((passed + 1))
echo 'PASS: external-launcher (external mode and input preserved)'

new_fixture cyclic-launcher
ln -s App.png "$app_dir/.DirIcon"
rm "$app_dir/AppRun"
ln -s launcher-cycle "$app_dir/AppRun"
ln -s AppRun "$app_dir/launcher-cycle"
pack_fixture
if bash "$repair_script" "$image" > "$case_dir/repair.log" 2>&1; then
  fail 'accepted a cyclic launcher link'
fi
[[ "$(sha256sum "$image" | cut -d ' ' -f1)" == "$before_hash" ]] || fail 'cyclic launcher failure changed the input'
passed=$((passed + 1))
echo 'PASS: cyclic-launcher (input preserved)'

new_fixture embedded-extraction-failure
runtime_extract_fails=true
pack_fixture
if bash "$repair_script" "$image" > "$case_dir/repair.log" 2>&1; then
  fail 'accepted a runtime that reports an offset but cannot extract its image'
fi
grep -q 'Embedded AppImage runtime extraction failed' "$case_dir/repair.log" || fail 'fixture failed before embedded runtime extraction'
[[ "$(sha256sum "$image" | cut -d ' ' -f1)" == "$before_hash" ]] || fail 'embedded extraction failure replaced the input'
[[ "$(stat -c %a "$image")" == 751 ]] || fail 'embedded extraction failure changed input mode'
[[ -z "$(find "$case_dir" -maxdepth 1 -name '.appimage-diricon.*' -print -quit)" ]] || fail 'embedded extraction failure left a temporary replacement'
passed=$((passed + 1))
echo 'PASS: embedded-extraction-failure (input preserved)'

echo "Passed $passed AppImage metadata/permission regression cases"
