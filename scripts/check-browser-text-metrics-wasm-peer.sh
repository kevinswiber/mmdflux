#!/usr/bin/env bash
# Fail if @mmds/browser-text-metrics' peerDependency range for @mmds/wasm
# does not cover the wasm crate's current version in
# crates/mmdflux-wasm/Cargo.toml. The cocogitto bump hook
# (bump-browser-text-metrics-wasm-peer.sh) keeps the two in sync during
# release; this script is the merge-time canary that catches a manual
# `cargo set-version` bump bypassing the release flow.
#
# Only supports the caret-range shape the bump hook writes (`^X.Y.Z`).
# Anything else is rejected so a hand-edit cannot widen the range to
# something the bump flow does not know how to maintain.
#
# Usage:
#   scripts/check-browser-text-metrics-wasm-peer.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CARGO_TOML="${REPO_ROOT}/crates/mmdflux-wasm/Cargo.toml"
PACKAGE_JSON="${REPO_ROOT}/packages/mmds-browser-text-metrics/package.json"

if [[ ! -f "${CARGO_TOML}" ]]; then
  echo "missing crate manifest: ${CARGO_TOML}" >&2
  exit 1
fi

if [[ ! -f "${PACKAGE_JSON}" ]]; then
  echo "missing package manifest: ${PACKAGE_JSON}" >&2
  exit 1
fi

WASM_VERSION="$(awk '
  /^\[package\]/ {in_pkg = 1; next}
  /^\[/ && in_pkg {in_pkg = 0}
  in_pkg && /^version[[:space:]]*=/ {
    sub(/^version[[:space:]]*=[[:space:]]*"/, "")
    sub(/".*/, "")
    print
    exit
  }
' "${CARGO_TOML}")"

if [[ -z "${WASM_VERSION}" ]]; then
  echo "could not read version from ${CARGO_TOML}" >&2
  exit 1
fi

PEER_RANGE="$(node -p \
  "require('${PACKAGE_JSON}').peerDependencies['@mmds/wasm']")"

if [[ -z "${PEER_RANGE}" || "${PEER_RANGE}" == "undefined" ]]; then
  echo "no peerDependencies['@mmds/wasm'] in ${PACKAGE_JSON}" >&2
  exit 1
fi

# semver-compare via node so we don't reimplement caret semantics in bash.
node - "${WASM_VERSION}" "${PEER_RANGE}" <<'NODE'
const [wasmVersion, peerRange] = process.argv.slice(2);
const caret = /^\^(\d+)\.(\d+)\.(\d+)$/;
const m = caret.exec(peerRange);
if (!m) {
  console.error(
    `peerDependencies['@mmds/wasm'] must be a caret range "^X.Y.Z" (got ${peerRange}).`,
  );
  process.exit(1);
}
const [, rMajor, rMinor, rPatch] = m.map(Number);
const v = /^(\d+)\.(\d+)\.(\d+)/.exec(wasmVersion);
if (!v) {
  console.error(`crate version is not semver: ${wasmVersion}`);
  process.exit(1);
}
const [, vMajor, vMinor, vPatch] = v.map(Number);
// Caret semantics for a non-zero major: covers >=X.Y.Z and <X+1.0.0.
const covers =
  rMajor === vMajor &&
  (vMinor > rMinor ||
    (vMinor === rMinor && vPatch >= rPatch));
if (!covers) {
  console.error(
    `peerDependencies['@mmds/wasm']=${peerRange} does not cover wasm crate ${wasmVersion}; run scripts/bump-browser-text-metrics-wasm-peer.sh.`,
  );
  process.exit(1);
}
console.log(
  `ok: peerDependencies['@mmds/wasm']=${peerRange} covers wasm crate ${wasmVersion}`,
);
NODE
