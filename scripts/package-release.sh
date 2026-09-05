#!/bin/sh
set -eu

fail() {
  printf '%s\n' "agent-lowmem package: $1" >&2
  exit 2
}

[ "$#" -eq 3 ] || fail "expected version, binary, and output directory"

version=$1
binary=$2
output=$3

case "$version" in
  *[!0-9.]* | .* | *. | *..* | *.*.*.*) fail "invalid version" ;;
esac

saved_ifs=$IFS
IFS=.
set -- $version
IFS=$saved_ifs
[ "$#" -eq 3 ] || fail "invalid version"
for component do
  case "$component" in
    0) ;;
    [1-9]*)
      case "$component" in
        *[!0-9]*) fail "invalid version" ;;
      esac
      ;;
    *) fail "invalid version" ;;
  esac
done

[ -f "$binary" ] && [ -x "$binary" ] || fail "binary must be a regular executable file"

repository=$(CDPATH= cd "$(dirname "$0")/.." && pwd -P)
[ -f "$repository/LICENSE.md" ] || fail "license is unavailable"
[ -f "$repository/README.md" ] || fail "readme is unavailable"

mkdir -p "$output" || fail "could not create output directory"
output=$(CDPATH= cd "$output" && pwd -P) || fail "invalid output directory"
[ "$output" != "$repository" ] || fail "output directory cannot be the repository root"

archive="agent-lowmem-v${version}-aarch64-apple-darwin.tar.gz"
archive_path="$output/$archive"
checksum_path="$output/SHA256SUMS"

rm -f "$archive_path" "$checksum_path"

stage=$(mktemp -d "${TMPDIR:-/tmp}/agent-lowmem-package.XXXXXX") || fail "could not create staging directory"
trap 'rm -rf "$stage"' EXIT HUP INT TERM

install -m 0755 "$binary" "$stage/agent-lowmem"
install -m 0644 "$repository/LICENSE.md" "$stage/LICENSE.md"
install -m 0644 "$repository/README.md" "$stage/README.md"
TZ=UTC touch -t 197001010000.00 "$stage/agent-lowmem" "$stage/LICENSE.md" "$stage/README.md"

COPYFILE_DISABLE=1 tar \
  --format ustar \
  --uid 0 \
  --gid 0 \
  --uname root \
  --gname wheel \
  --options 'gzip:!timestamp' \
  -C "$stage" \
  -czf "$archive_path" \
  agent-lowmem LICENSE.md README.md

(cd "$output" && shasum -a 256 "$archive" > SHA256SUMS)
(cd "$output" && shasum -a 256 -c SHA256SUMS)
