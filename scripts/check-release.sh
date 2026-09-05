#!/bin/sh
set -eu

fail() {
  printf '%s\n' "agent-lowmem release: $1" >&2
  exit 2
}

valid_version() {
  candidate=$1
  case "$candidate" in
    *[!0-9.]* | .* | *. | *..* | *.*.*.*) return 1 ;;
  esac
  saved_ifs=$IFS
  IFS=.
  set -- $candidate
  IFS=$saved_ifs
  [ "$#" -eq 3 ] || return 1
  for component do
    case "$component" in
      0) ;;
      [1-9]*)
        case "$component" in
          *[!0-9]*) return 1 ;;
        esac
        ;;
      *) return 1 ;;
    esac
  done
}

[ "$#" -eq 4 ] || fail "expected version, cargo-audit, output directory, and evidence file"

version=$1
cargo_audit=$2
output=$3
evidence=$4

valid_version "$version" || fail "invalid version"
[ -f "$cargo_audit" ] && [ -x "$cargo_audit" ] || fail "cargo-audit must be a regular executable file"

repository=$(CDPATH= cd "$(dirname "$0")/.." && pwd -P)
[ -d "$repository/.git" ] || fail "repository metadata is unavailable"

mkdir -p "$output" || fail "could not create output directory"
output=$(CDPATH= cd "$output" && pwd -P) || fail "invalid output directory"
[ "$output" != "$repository" ] || fail "output directory cannot be the repository root"

evidence_parent=$(dirname "$evidence")
[ -d "$evidence_parent" ] || fail "evidence parent must already exist"
evidence_parent=$(CDPATH= cd "$evidence_parent" && pwd -P) || fail "invalid evidence parent"
evidence="$evidence_parent/$(basename "$evidence")"
case "$evidence" in
  "$repository" | "$repository"/*) fail "evidence must be outside the repository" ;;
esac
[ ! -e "$evidence" ] && [ ! -L "$evidence" ] || fail "evidence file already exists"

[ "$(uname -m)" = "arm64" ] || fail "release checks require an ARM64 host"

cd "$repository"
[ -z "$(git status --porcelain=v1 --untracked-files=all)" ] || fail "worktree must be clean"
head=$(git rev-parse --verify HEAD 2>/dev/null) || fail "HEAD is unavailable"
remote_main=$(git rev-parse --verify refs/remotes/origin/main 2>/dev/null) || fail "origin/main is unavailable"
[ "$head" = "$remote_main" ] || fail "main must match origin/main"

metadata=$(cargo metadata --locked --no-deps --format-version 1 2>/dev/null) || fail "Cargo metadata failed"
package_version=$(printf '%s' "$metadata" | jq -er '.packages | select(length == 1) | .[0].version' 2>/dev/null) || fail "package metadata is invalid"
[ "$package_version" = "$version" ] || fail "package version does not match"

cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 -- --test-threads=1
cargo test --release --test doctor_budget -- --ignored --test-threads=1
cargo test --release --test managed_files_budget -- --ignored --test-threads=1
cargo test --release --test run_budget -- --ignored --test-threads=1

full_metadata=$(cargo metadata --locked --format-version 1)
license_result=$(printf '%s' "$full_metadata" | jq -er '
  ["MIT","Unlicense OR MIT","BSD-3-Clause","MIT OR Apache-2.0","Apache-2.0 OR MIT","Apache-2.0/MIT","Apache-2.0","ISC","Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT","MIT/Apache-2.0","Unlicense/MIT","(MIT OR Apache-2.0) AND Unicode-3.0"] as $allowed |
  [.packages[] | select(.source != null) | . as $package | select(($package.license == null) or (($allowed | index($package.license)) == null))] as $rejected |
  if ($rejected | length) == 0 then "pass" else error("external dependency license rejected") end
') || fail "dependency license audit failed"
[ "$license_result" = "pass" ] || fail "dependency license audit failed"

"$cargo_audit" audit --deny warnings --file Cargo.lock
cargo build --release --locked -j 1

binary=target/release/agent-lowmem
[ -f "$binary" ] && [ -x "$binary" ] || fail "release binary is unavailable"
file "$binary" | grep -q 'arm64' || fail "release binary is not ARM64"
binary_bytes=$(stat -f '%z' "$binary") || fail "could not measure release binary"
[ "$binary_bytes" -le 12582912 ] || fail "release binary exceeds 12 MiB"
[ "$("$binary" --version)" = "agent-lowmem $version" ] || fail "version smoke test failed"
"$binary" doctor >/dev/null

scripts/package-release.sh "$version" "$binary" "$output"
archive="agent-lowmem-v${version}-aarch64-apple-darwin.tar.gz"
(cd "$output" && shasum -a 256 -c SHA256SUMS)
members=$(tar -tzf "$output/$archive")
[ "$members" = "agent-lowmem
LICENSE.md
README.md" ] || fail "archive inventory is invalid"
archive_bytes=$(stat -f '%z' "$output/$archive") || fail "could not measure release archive"
package_count=$(printf '%s' "$full_metadata" | jq -er '.packages | length') || fail "could not count packages"

umask 077
evidence_tmp=$(mktemp "$evidence_parent/.agent-lowmem-release.XXXXXX") || fail "could not create evidence file"
trap 'rm -f "$evidence_tmp"' EXIT HUP INT TERM
{
  printf 'schema=agent-lowmem-local-release-v1\n'
  printf 'verified_at=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  printf 'commit=%s\n' "$head"
  printf 'architecture=arm64\n'
  printf 'macos=%s\n' "$(sw_vers -productVersion)"
  printf 'rustc=%s\n' "$(rustc --version)"
  printf 'cargo=%s\n' "$(cargo --version)"
  printf 'cargo_audit=%s\n' "$("$cargo_audit" --version | head -n 1)"
  printf 'package_count=%s\n' "$package_count"
  printf 'licenses=pass\n'
  printf 'advisories=pass\n'
  printf 'active_tests=pass\n'
  printf 'release_only_tests=pass\n'
  printf 'binary_bytes=%s\n' "$binary_bytes"
  printf 'archive=%s\n' "$archive"
  printf 'archive_bytes=%s\n' "$archive_bytes"
  printf 'archive_members=agent-lowmem,LICENSE.md,README.md\n'
  printf 'checksum=pass\n'
  printf 'status=pass\n'
} > "$evidence_tmp"
chmod 0600 "$evidence_tmp"
mv "$evidence_tmp" "$evidence"
trap - 0 HUP INT TERM

printf '%s\n' "agent-lowmem release: verified"
