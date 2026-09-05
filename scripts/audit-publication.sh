#!/bin/sh
set -eu

fail() {
  printf '%s\n' "agent-lowmem publication audit: $1" >&2
  exit 2
}

[ "$#" -eq 2 ] || fail "expected gitleaks executable and evidence file"

gitleaks=$1
evidence=$2

[ -f "$gitleaks" ] && [ -x "$gitleaks" ] || fail "gitleaks must be a regular executable file"

repository=$(CDPATH= cd "$(dirname "$0")/.." && pwd -P)
[ -d "$repository/.git" ] || fail "repository metadata is unavailable"

evidence_parent=$(dirname "$evidence")
[ -d "$evidence_parent" ] || fail "evidence parent must already exist"
evidence_parent=$(CDPATH= cd "$evidence_parent" && pwd -P) || fail "invalid evidence parent"
evidence="$evidence_parent/$(basename "$evidence")"
case "$evidence" in
  "$repository" | "$repository"/*) fail "evidence must be outside the repository" ;;
esac
[ ! -e "$evidence" ] && [ ! -L "$evidence" ] || fail "evidence file already exists"

cd "$repository"
status=$(git status --porcelain=v1 --untracked-files=all 2>/dev/null) || fail "worktree state is unavailable"
[ -z "$status" ] || fail "worktree must be clean"
head=$(git rev-parse --verify HEAD 2>/dev/null) || fail "HEAD is unavailable"
remote_main=$(git rev-parse --verify refs/remotes/origin/main 2>/dev/null) || fail "origin/main is unavailable"
[ "$head" = "$remote_main" ] || fail "main must match origin/main"
[ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = "false" ] || fail "shallow repositories are not auditable"

git fsck --full >/dev/null 2>&1 || fail "Git object integrity check failed"

if git rev-list --objects --all | awk 'NF > 1 { sub(/^[^ ]+ /, ""); print }' | while IFS= read -r path; do
  name=${path##*/}
  case "$name" in
    .gitmodules | .npmrc | .pypirc | .env | id_rsa | id_ed25519 | credentials.json | *.pem | *.key | *.p12 | *.pfx | *.kdbx | service-account*.json)
      exit 1
      ;;
    .env.*)
      case "$name" in
        .env.example | .env.sample | .env.template) ;;
        *) exit 1 ;;
      esac
      ;;
  esac
done
then
  :
else
  fail "publication history contains a prohibited tracked path"
fi

for commit in $(git rev-list --all); do
  if git grep -I -q -e '^version https://git-lfs.github.com/spec/v1$' "$commit" -- 2>/dev/null; then
    fail "publication history contains a Git LFS pointer"
  else
    grep_status=$?
    [ "$grep_status" -eq 1 ] || fail "could not inspect publication history"
  fi
done

scanner_log=$(mktemp "${TMPDIR:-/tmp}/agent-lowmem-gitleaks.XXXXXX") || fail "could not create scanner log"
trap 'rm -f "$scanner_log"' EXIT HUP INT TERM

set +e
printf 'token = "ghp_%s%s"\n' 'aBcDeFgHiJkLmNoPqRsT' 'uVwXyZ0123456789' |
  "$gitleaks" stdin --redact --no-banner > /dev/null 2>"$scanner_log"
canary_status=$?
set -e
[ "$canary_status" -eq 1 ] || fail "secret scanner failed its detection self-test"

if ! "$gitleaks" detect --source . --redact --no-banner --exit-code 1 --log-opts=--all > /dev/null 2>"$scanner_log"; then
  fail "secret scanner reported a finding or execution failure"
fi

commit_count=$(git rev-list --all --count 2>/dev/null) || fail "could not count commits"
ref_count=$(git for-each-ref --format='%(refname)' refs/heads refs/remotes refs/tags 2>/dev/null | wc -l | tr -d ' ')
object_path_count=$(git rev-list --objects --all 2>/dev/null | awk 'NF > 1 { count += 1 } END { print count + 0 }')

umask 077
evidence_tmp=$(mktemp "$evidence_parent/.agent-lowmem-publication.XXXXXX") || fail "could not create evidence file"
trap 'rm -f "$scanner_log" "$evidence_tmp"' EXIT HUP INT TERM
{
  printf 'schema=agent-lowmem-publication-audit-v1\n'
  printf 'verified_at=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  printf 'commit=%s\n' "$head"
  printf 'commit_count=%s\n' "$commit_count"
  printf 'ref_count=%s\n' "$ref_count"
  printf 'object_path_count=%s\n' "$object_path_count"
  printf 'git_fsck=pass\n'
  printf 'shallow=false\n'
  printf 'submodules=0\n'
  printf 'lfs_pointers=0\n'
  printf 'suspicious_paths=0\n'
  printf 'scan=pass\n'
  printf 'status=pass\n'
} > "$evidence_tmp"
chmod 0600 "$evidence_tmp"
mv "$evidence_tmp" "$evidence"
rm -f "$scanner_log"
trap - 0 HUP INT TERM

printf '%s\n' "agent-lowmem publication audit: verified"
