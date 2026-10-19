#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "Usage: $0 <AppImage>..." >&2
  exit 2
fi

for tool in unsquashfs mksquashfs readlink od; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "$tool is required to check AppImage icons" >&2
    exit 1
  }
done

# Resolve every link within the extracted AppDir. A successful `test -e` alone
# also accepts absolute links into the original, still-existing build directory.
resolve_portable_file() {
  local root="$1" relative="$2" resolved="" component candidate target
  local links=0
  local -a pending target_parts
  [[ "$relative" != /* && "$relative" != *$'\n'* ]] || return 1
  IFS=/ read -r -a pending <<< "$relative"
  while ((${#pending[@]})); do
    component="${pending[0]}"
    pending=("${pending[@]:1}")
    case "$component" in
      ''|.) continue ;;
      ..)
        [[ -n "$resolved" ]] || return 1
        if [[ "$resolved" == */* ]]; then
          resolved="${resolved%/*}"
        else
          resolved=""
        fi
        continue
        ;;
    esac
    candidate="$root/${resolved:+$resolved/}$component"
    if [[ -L "$candidate" ]]; then
      links=$((links + 1))
      ((links <= 40)) || return 1
      target="$(readlink -- "$candidate")" || return 1
      [[ "$target" != /* && "$target" != *$'\n'* ]] || return 1
      IFS=/ read -r -a target_parts <<< "$target"
      pending=("${target_parts[@]}" "${pending[@]}")
    else
      resolved="${resolved:+$resolved/}$component"
      if ((${#pending[@]})); then
        [[ -d "$candidate" ]] || return 1
      fi
    fi
  done
  [[ -f "$root/$resolved" ]] || return 1
  printf '%s\n' "$root/$resolved"
}

portable_png() {
  local png
  png="$(resolve_portable_file "$1" "$2")" || return 1
  [[ "$(od -An -v -tx1 -N8 -- "$png" | tr -d '[:space:]')" == 89504e470d0a1a0a ]]
}

verify_metadata() {
  local root="$1" desktop
  portable_png "$root" .DirIcon || return 1
  while IFS= read -r -d '' desktop; do
    resolve_portable_file "$root" "${desktop##*/}" >/dev/null || return 1
  done < <(find "$root" -maxdepth 1 -name '*.desktop' -print0)
}

permission_issue() {
  find "$1" \( \( -type d ! -perm -0555 \) -o \
    \( -type f \( ! -perm -0444 -o \( -perm /0111 ! -perm -0111 \) \) \) \) -print -quit
}

normalize_permissions() {
  local root="$1" launcher target
  [[ -d "$root" && ! -L "$root" ]] || return 1
  if [[ -n "$(permission_issue "$root")" ]]; then
    # The extraction root is a real directory. GNU chmod ignores links found
    # during recursion, so package links cannot change files outside AppDir.
    chmod -R a+rX -- "$root" || return 1
    changed=true
  fi
  for launcher in AppRun AppRun.wrapped; do
    if [[ "$launcher" == AppRun || -e "$root/$launcher" || -L "$root/$launcher" ]]; then
      target="$(resolve_portable_file "$root" "$launcher")" || {
        echo "Missing or nonportable AppImage launcher: $launcher" >&2
        return 1
      }
      if [[ -n "$(find "$target" ! -perm -0111 -print -quit)" ]]; then
        # Resolution removed every link component before chmod follows a path.
        chmod a+x -- "$target" || return 1
        changed=true
      fi
    fi
  done
}

verify_permissions() {
  local root="$1" launcher target
  [[ -z "$(permission_issue "$root")" ]] || return 1
  for launcher in AppRun AppRun.wrapped; do
    if [[ "$launcher" == AppRun || -e "$root/$launcher" || -L "$root/$launcher" ]]; then
      target="$(resolve_portable_file "$root" "$launcher")" || return 1
      [[ -z "$(find "$target" ! -perm -0111 -print -quit)" ]] || return 1
    fi
  done
}

squashfs_compression() {
  LC_ALL=C unsquashfs -s -offset "$2" "$1" | sed -n 's/^Compression[[:space:]]*//p'
}

work_dir=""
repaired_image=""
cleanup() {
  if [[ -n "$work_dir" ]]; then rm -rf -- "$work_dir"; fi
  if [[ -n "$repaired_image" ]]; then rm -f -- "$repaired_image"; fi
}
trap cleanup EXIT

for image in "$@"; do
  [[ -f "$image" ]] || { echo "AppImage not found: $image" >&2; exit 1; }
  [[ "$image" == */* ]] || image="./$image"
  offset="$("$image" --appimage-offset)"
  [[ "$offset" =~ ^[0-9]+$ ]] || { echo "Invalid AppImage offset: $image" >&2; exit 1; }

  work_dir="$(mktemp -d)"
  unsquashfs -quiet -offset "$offset" -dest "$work_dir/AppDir" "$image"

  changed=false
  compression="$(squashfs_compression "$image" "$offset")"
  [[ -n "$compression" ]] || { echo "Cannot determine AppImage compression: $image" >&2; exit 1; }
  # Modern Tauri runtimes support zlib (SquashFS gzip) and zstd, but not xz.
  # Also normalize older already-repaired xz images instead of accepting them.
  case "$compression" in
    gzip|zstd) ;;
    *) changed=true ;;
  esac
  normalize_permissions "$work_dir/AppDir" || {
    echo "AppImage permission normalization failed: $image" >&2
    exit 1
  }
  if ! portable_png "$work_dir/AppDir" .DirIcon; then
    icon=""
    while IFS= read -r -d '' candidate; do
      if portable_png "$work_dir/AppDir" "${candidate##*/}"; then
        icon="${candidate##*/}"
        break
      fi
    done < <(find "$work_dir/AppDir" -maxdepth 1 -name '*.png' -print0)
    [[ -n "$icon" ]] || { echo "No portable root PNG icon in $image" >&2; exit 1; }
    ln -sfnT -- "$icon" "$work_dir/AppDir/.DirIcon"
    changed=true
  fi

  # Older bundlers can leave root desktop links pointing at the build host too.
  while IFS= read -r -d '' desktop; do
    desktop_name="${desktop##*/}"
    if ! resolve_portable_file "$work_dir/AppDir" "$desktop_name" >/dev/null; then
      desktop_target="usr/share/applications/$desktop_name"
      resolve_portable_file "$work_dir/AppDir" "$desktop_target" >/dev/null || {
        echo "No portable desktop entry for $desktop_name in $image" >&2
        exit 1
      }
      ln -sfnT -- "$desktop_target" "$desktop"
      changed=true
    fi
  done < <(find "$work_dir/AppDir" -maxdepth 1 -name '*.desktop' -print0)

  candidate_image="$image"
  if [[ "$changed" == true ]]; then
    head -c "$offset" "$image" > "$work_dir/runtime"
    mksquashfs "$work_dir/AppDir" "$work_dir/filesystem.squashfs" -noappend -comp gzip -quiet >/dev/null
    repaired_image="$(mktemp "$(dirname "$image")/.appimage-diricon.XXXXXX")"
    cat "$work_dir/runtime" "$work_dir/filesystem.squashfs" > "$repaired_image"
    chmod --reference="$image" "$repaired_image"
    candidate_image="$repaired_image"
  fi

  # Verify a fresh extraction before replacing the input. Link resolution must
  # stay within this copy even when the original build directory still exists.
  unsquashfs -quiet -offset "$offset" -dest "$work_dir/verify" "$candidate_image" >/dev/null
  case "$(squashfs_compression "$candidate_image" "$offset")" in
    gzip|zstd) ;;
    *) echo "AppImage runtime does not support the SquashFS compression: $image" >&2; exit 1 ;;
  esac
  if ! verify_metadata "$work_dir/verify" || ! verify_permissions "$work_dir/verify"; then
    echo "AppImage portable metadata/permission verification failed: $image" >&2
    exit 1
  fi

  # System unsquashfs may support codecs that the embedded AppImage runtime
  # cannot read. Exercise that runtime before replacing the original image.
  runtime_image="$(readlink -f -- "$candidate_image")"
  mkdir "$work_dir/runtime-verify"
  if ! (cd "$work_dir/runtime-verify" && "$runtime_image" --appimage-extract) > "$work_dir/runtime-extract.log" 2>&1; then
    cat "$work_dir/runtime-extract.log" >&2
    echo "Embedded AppImage runtime extraction failed: $image" >&2
    exit 1
  fi
  runtime_root="$work_dir/runtime-verify/squashfs-root"
  # The embedded extractor creates directories with its own private mode 0700;
  # archive permissions are checked on the independent unsquashfs copy above.
  if [[ ! -d "$runtime_root" || -L "$runtime_root" ]] || \
    ! verify_metadata "$runtime_root"; then
    echo "Embedded AppImage runtime metadata verification failed: $image" >&2
    exit 1
  fi

  if [[ "$changed" == true ]]; then
    mv -f -- "$repaired_image" "$image"
    repaired_image=""
    echo "Repaired portable AppImage metadata in $image"
  else
    echo "Portable AppImage metadata verified in $image"
  fi
  rm -rf -- "$work_dir"
  work_dir=""
done
