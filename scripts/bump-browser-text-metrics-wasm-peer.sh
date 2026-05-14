#!/usr/bin/env bash
# Cocogitto pre-bump hook for @mmds/browser-text-metrics.
#
# The package peer-depends on @mmds/wasm. @mmds/wasm's version is the
# Rust crate version in crates/mmdflux-wasm/Cargo.toml, which the
# mmdflux package's own pre-bump hook updates first (cargo set-version)
# — so by the time this script runs, the Cargo.toml carries the new
# target version.
#
# Mirrors how @mmds/excalidraw and @mmds/tldraw pin @mmds/core via
# `npm install "@mmds/core@^$(node -p require('../mmds-core/package.json').version)"`,
# adapted for two differences:
#   1. @mmds/wasm is a peer dependency, not a regular dependency.
#   2. @mmds/wasm is not a workspace sibling, so plain `npm install`
#      would try to fetch from the registry. We update package.json
#      via `npm pkg set` and patch packages/package-lock.json directly
#      via Node so the lockfile change is byte-targeted to the peer
#      dependency range — `npm install --package-lock-only` is not
#      idempotent (it can rewrite unrelated optional fields like `libc`
#      depending on npm's current view of the registry).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CARGO_TOML="${REPO_ROOT}/crates/mmdflux-wasm/Cargo.toml"

if [[ ! -f "${CARGO_TOML}" ]]; then
  echo "missing crate manifest: ${CARGO_TOML}" >&2
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

PACKAGE_DIR="${REPO_ROOT}/packages/mmds-browser-text-metrics"
LOCKFILE="${REPO_ROOT}/packages/package-lock.json"
RANGE="^${WASM_VERSION}"

(
  cd "${PACKAGE_DIR}"
  npm pkg set "peerDependencies.@mmds/wasm=${RANGE}"
)

# Surgically patch the workspace lockfile entry so the recorded
# peerDependency range matches package.json. We update both the
# workspace-relative key (`packages["mmds-browser-text-metrics"]`)
# and the absolute installed-path key (`node_modules/...`) if the
# latter exists, leaving every other key — including npm-rewritten
# `libc` / `os` metadata — untouched.
RANGE="${RANGE}" LOCKFILE="${LOCKFILE}" node <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const lockPath = process.env.LOCKFILE;
const range = process.env.RANGE;
const raw = fs.readFileSync(lockPath, "utf8");
const trailingNewline = raw.endsWith("\n") ? "\n" : "";
const lock = JSON.parse(raw);
const targets = [
  "mmds-browser-text-metrics",
  "node_modules/@mmds/browser-text-metrics",
];
let patched = 0;
for (const key of targets) {
  const entry = lock.packages && lock.packages[key];
  if (!entry) continue;
  entry.peerDependencies = entry.peerDependencies || {};
  if (entry.peerDependencies["@mmds/wasm"] !== range) {
    entry.peerDependencies["@mmds/wasm"] = range;
    patched += 1;
  }
}
fs.writeFileSync(lockPath, JSON.stringify(lock, null, 2) + trailingNewline);
console.error(`patched ${patched} lockfile entry/entries`);
NODE

echo "set peerDependency @mmds/wasm to ${RANGE}"
