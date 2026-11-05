#!/usr/bin/env bash
# Puts one Release's packages on the Nexus Mods page (docs/RELEASE.md):
#   .github/nexus-upload.sh <version> <package>...
# Each package (kjk-v0.6.0-linux-x86_64.tar.gz) becomes a new version of its
# OS's file on the page, the file named by the environment variable
# NEXUS_FILE_<PLATFORM> (NEXUS_FILE_LINUX_X86_64): the API's id for the file,
# or the file_id in one of its versions' download links on the site. With no
# such variable the package is added as a new main file and its id is
# printed, to be saved so the next release updates it.
#
# NEXUS_API_KEY is the owner's key (nexusmods.com/settings/api-keys),
# NEXUS_MOD_ID the mod's id (not the 1911 in its URL), NEXUS_GAME the game's
# name in the site's URLs. The API: https://api-docs.nexusmods.com/
set -euo pipefail

api=https://api.nexusmods.com/v3
version=${1#v}
shift

call() { # method path [json]; on failure Nexus's answer goes to stderr
  local out
  if ! out=$(curl --silent --show-error --fail-with-body -X "$1" "$api$2" \
    -H "apikey: $NEXUS_API_KEY" -H "Content-Type: application/json" \
    ${3:+--data "$3"}); then
    echo "$1 $2 failed: $out" >&2
    return 1
  fi
  printf '%s\n' "$out"
}

# The API's id for a file, given that id or a version's game-scoped id.
file_of() {
  if call GET "/mod-files/$1" >/dev/null 2>&1; then
    echo "$1"
  else
    call GET "/games/$NEXUS_GAME/mod-file-versions/$1" | jq -er .data.file.id
  fi
}

label() {
  case "$1" in
    macos-aarch64) echo "macOS (Apple silicon)" ;;
    macos-x86_64) echo "macOS (Intel)" ;;
    windows-x86_64) echo "Windows (x86-64)" ;;
    linux-x86_64) echo "Linux (x86-64)" ;;
    *) echo "$1" ;;
  esac
}

for package in "$@"; do
  filename=$(basename "$package")
  platform=${filename#kjk-v"$version"-}
  platform=${platform%.tar.gz}
  platform=${platform%.zip}
  variable=NEXUS_FILE_$(tr 'a-z-' 'A-Z_' <<<"$platform")
  file_id=${!variable:-}
  if [ -n "$file_id" ]; then
    file_id=$(file_of "$file_id") || {
      echo "::error::$variable names no file or file version on Nexus"
      exit 1
    }
  fi
  size=$(stat -c %s "$package")
  if (( size > 100 * 1024 * 1024 )); then
    echo "::error::$filename is over 100 MiB, which needs Nexus's multipart upload (not written yet)"
    exit 1
  fi
  md5=$(md5sum "$package" | cut -d' ' -f1)
  echo "== $filename ($size bytes) -> ${file_id:-a new file}"

  upload=$(call POST /uploads "$(jq -n --arg f "$filename" --argjson s "$size" --arg m "$md5" \
    '{filename: $f, size_bytes: $s, md5: $m}')")
  upload_id=$(jq -r .data.id <<<"$upload")
  curl --silent --show-error --fail-with-body -X PUT --upload-file "$package" \
    -H "Content-Type: application/octet-stream" \
    -H "Content-Disposition: attachment; filename=\"$filename\"" \
    -H "Content-MD5: $(openssl dgst -md5 -binary "$package" | base64)" \
    "$(jq -r .data.presigned_url <<<"$upload")"
  call POST "/uploads/$upload_id/finalise" >/dev/null

  for _ in $(seq 60); do
    state=$(call GET "/uploads/$upload_id" | jq -r .data.state)
    [ "$state" = available ] && break
    sleep 5
  done
  if [ "$state" != available ]; then
    echo "::error::upload $upload_id for $filename is still '$state' after five minutes"
    exit 1
  fi

  body=$(jq -n --arg u "$upload_id" --arg n "kjk $(label "$platform")" --arg v "$version" \
    --arg d "kjk $version for $(label "$platform"). Read README.txt in the package first." \
    '{upload_id: $u, name: $n, version: $v, description: $d, file_category: "main", update_mod_version: true}')
  if [ -n "$file_id" ]; then
    call POST "/mod-files/$file_id/versions" "$body" | jq -c .data
  else
    created=$(call POST /mod-files "$(jq --arg m "$NEXUS_MOD_ID" '. + {mod_id: $m}' <<<"$body")")
    jq -c .data <<<"$created"
    echo "::notice::$filename is a new file on the page; set the repository variable $variable to $(jq -r .data.id <<<"$created") so the next release updates it"
  fi
done
