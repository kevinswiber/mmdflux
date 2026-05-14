#!/usr/bin/env bash
# Smoke test for the `Resolve package and version from tag` step in
# .github/workflows/packages-release.yml. Run this locally before tagging
# a release to confirm the bash switch maps the tag to the expected
# package directory, npm package name, and version.
#
# Usage:
#   scripts/test-release-tag-resolver.sh
#
# The script extracts each known tag prefix, runs the resolver, and
# checks the output. Add a new case here whenever you add a tag prefix
# to packages-release.yml.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORKFLOW="${REPO_ROOT}/.github/workflows/packages-release.yml"

if [[ ! -f "${WORKFLOW}" ]]; then
  echo "missing workflow: ${WORKFLOW}" >&2
  exit 1
fi

# Reuse the actual bash switch from the workflow rather than duplicating
# it here. Extract everything between the `if [[` and the closing `fi`
# that follows `Unknown tag format`. The script then sources that block
# in a subshell with `TAG` set.
RESOLVER="$(awk '
  /^[[:space:]]*if \[\[ "\$TAG" == mmds-/ {capture = 1}
  capture {print; last_match_line = NR}
  capture && /^[[:space:]]*fi[[:space:]]*$/ && last_match_line == NR {capture = 0}
' "${WORKFLOW}")"

if [[ -z "${RESOLVER}" ]]; then
  echo "could not extract resolver block from ${WORKFLOW}" >&2
  exit 1
fi

run_resolver() {
  local tag="$1"
  local github_output
  github_output="$(mktemp)"
  (
    export TAG="${tag}"
    export GITHUB_OUTPUT="${github_output}"
    # Run the resolver in a nested subshell so `exit 1` on unknown tags
    # does not abort this function; the outer subshell still records
    # the inner exit code.
    (eval "${RESOLVER}") 2>/dev/null
    echo "__exit_code=$?"
  )
  cat "${github_output}"
  rm -f "${github_output}"
}

expect_output() {
  local label="$1" actual="$2" pattern="$3"
  if ! grep -qE "${pattern}" <<<"${actual}"; then
    echo "FAIL ${label}: expected pattern '${pattern}' in:" >&2
    printf '%s\n' "${actual}" >&2
    exit 1
  fi
  echo "ok ${label}: ${pattern}"
}

CASES=(
  "mmds-core-v0.2.5|pkg_dir=packages/mmds-core|@mmds/core|version=0.2.5"
  "mmds-excalidraw-v1.2.3|pkg_dir=packages/mmds-excalidraw|@mmds/excalidraw|version=1.2.3"
  "mmds-tldraw-v0.9.0|pkg_dir=packages/mmds-tldraw|@mmds/tldraw|version=0.9.0"
  "mmds-browser-text-metrics-v0.1.0|pkg_dir=packages/mmds-browser-text-metrics|@mmds/browser-text-metrics|version=0.1.0"
)

for case in "${CASES[@]}"; do
  IFS='|' read -r tag dir name version <<<"${case}"
  out="$(run_resolver "${tag}")"
  expect_output "${tag} dir" "${out}" "^${dir}$"
  expect_output "${tag} npm" "${out}" "^npm_name=${name}$"
  expect_output "${tag} ver" "${out}" "^${version}$"
done

# Unknown tags must fail loudly.
out="$(run_resolver "not-a-known-tag-v1.0.0" || true)"
if ! grep -q "__exit_code=1" <<<"${out}"; then
  echo "FAIL unknown tag: resolver did not exit non-zero" >&2
  printf '%s\n' "${out}" >&2
  exit 1
fi
echo "ok unknown tag: resolver rejects unrecognized prefix"

echo "all tag resolver cases passed"
