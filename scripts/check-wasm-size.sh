#!/usr/bin/env bash
set -euo pipefail

# Size budgets are regression guardrails, not "match Mermaid.js" targets.
# Rationale source:
#   .gumbo/research/0042-wasm-web-playground/q3-performance-baseline.md
# Mermaid's minified bundle is ~2.7 MB; mmdflux release wasm is currently
# about ~1.9 MB raw / ~0.6 MB gzip. These thresholds preserve headroom while
# failing obvious size regressions. Tighten over time as size work lands.

WASM_WEB_OUT_DIR="${WASM_WEB_OUT_DIR:-target/wasm-pkg-web}"
WASM_BUNDLER_OUT_DIR="${WASM_BUNDLER_OUT_DIR:-target/wasm-pkg-bundler}"
WASM_MAX_BYTES="${WASM_MAX_BYTES:-2097152}"
WASM_GZIP_MAX_BYTES="${WASM_GZIP_MAX_BYTES:-700000}"
WASM_CARGO_PROFILE_RELEASE_OPT_LEVEL="${WASM_CARGO_PROFILE_RELEASE_OPT_LEVEL:-z}"
WASM_CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${WASM_CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}"
WASM_CARGO_PROFILE_RELEASE_LTO="${WASM_CARGO_PROFILE_RELEASE_LTO:-fat}"
WASM_CARGO_PROFILE_RELEASE_PANIC="${WASM_CARGO_PROFILE_RELEASE_PANIC:-abort}"

build_artifacts=1

usage() {
  cat <<'EOF'
Usage: scripts/check-wasm-size.sh [--no-build]

Builds release wasm artifacts for web and bundler targets (unless --no-build is set),
then enforces size budgets equivalent to .github/workflows/wasm-ci.yml.

Environment overrides:
  WASM_WEB_OUT_DIR      (default: target/wasm-pkg-web)
  WASM_BUNDLER_OUT_DIR  (default: target/wasm-pkg-bundler)
  WASM_MAX_BYTES        (default: 2097152)
  WASM_GZIP_MAX_BYTES   (default: 700000)
  WASM_CARGO_PROFILE_RELEASE_OPT_LEVEL     (default: z)
  WASM_CARGO_PROFILE_RELEASE_CODEGEN_UNITS (default: 1)
  WASM_CARGO_PROFILE_RELEASE_LTO           (default: fat)
  WASM_CARGO_PROFILE_RELEASE_PANIC         (default: abort)
EOF
}

wasm_pack_release_build() {
  env \
    CARGO_PROFILE_RELEASE_OPT_LEVEL="${WASM_CARGO_PROFILE_RELEASE_OPT_LEVEL}" \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${WASM_CARGO_PROFILE_RELEASE_CODEGEN_UNITS}" \
    CARGO_PROFILE_RELEASE_LTO="${WASM_CARGO_PROFILE_RELEASE_LTO}" \
    CARGO_PROFILE_RELEASE_PANIC="${WASM_CARGO_PROFILE_RELEASE_PANIC}" \
    wasm-pack build "$@"
}

while (($# > 0)); do
  case "$1" in
    --no-build)
      build_artifacts=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ((build_artifacts)); then
  if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "Error: wasm-pack not found in PATH." >&2
    exit 1
  fi

  echo "Building release wasm package (web target)..."
  wasm_pack_release_build crates/mmdflux-wasm --target web --release --out-dir "../../${WASM_WEB_OUT_DIR}"

  echo "Building release wasm package (bundler target)..."
  wasm_pack_release_build crates/mmdflux-wasm --target bundler --release --out-dir "../../${WASM_BUNDLER_OUT_DIR}"
fi

web_wasm="${WASM_WEB_OUT_DIR}/mmdflux_wasm_bg.wasm"
bundler_wasm="${WASM_BUNDLER_OUT_DIR}/mmdflux_wasm_bg.wasm"

for wasm in "$web_wasm" "$bundler_wasm"; do
  if [[ ! -f "$wasm" ]]; then
    echo "Expected wasm artifact missing: $wasm" >&2
    exit 1
  fi
  gzip -kf "$wasm"
done

format_int() {
  local value="$1"
  awk -v n="$value" 'BEGIN {
    n = sprintf("%.0f", n)
    out = ""
    while (length(n) > 3) {
      out = "," substr(n, length(n)-2) out
      n = substr(n, 1, length(n)-3)
    }
    print n out
  }'
}

overall_failed=0

report_target() {
  local label="$1"
  local wasm_file="$2"
  local gzip_file="${wasm_file}.gz"

  local wasm_bytes
  local gzip_bytes
  wasm_bytes=$(wc -c < "$wasm_file" | tr -d ' ')
  gzip_bytes=$(wc -c < "$gzip_file" | tr -d ' ')

  local wasm_kib
  local gzip_kib
  wasm_kib=$(awk "BEGIN {printf \"%.1f\", ${wasm_bytes}/1024}")
  gzip_kib=$(awk "BEGIN {printf \"%.1f\", ${gzip_bytes}/1024}")

  local wasm_bytes_fmt
  local gzip_bytes_fmt
  wasm_bytes_fmt="$(format_int "${wasm_bytes}")"
  gzip_bytes_fmt="$(format_int "${gzip_bytes}")"

  local status="PASS"
  if (( wasm_bytes > WASM_MAX_BYTES || gzip_bytes > WASM_GZIP_MAX_BYTES )); then
    status="FAIL"
    overall_failed=1
  fi

  printf '| %-8s | %14s | %9s | %14s | %9s | %-6s |\n' \
    "$label" \
    "$wasm_bytes_fmt" \
    "$wasm_kib" \
    "$gzip_bytes_fmt" \
    "$gzip_kib" \
    "$status"

  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf '| %s | %s | %s | %s | %s | %s |\n' \
      "$label" \
      "$wasm_bytes_fmt" \
      "$wasm_kib" \
      "$gzip_bytes_fmt" \
      "$gzip_kib" \
      "$status" >> "$GITHUB_STEP_SUMMARY"
  fi

  if (( wasm_bytes > WASM_MAX_BYTES )); then
    echo "${label} raw wasm size ${wasm_bytes} exceeds budget ${WASM_MAX_BYTES}" >&2
  fi

  if (( gzip_bytes > WASM_GZIP_MAX_BYTES )); then
    echo "${label} gzipped wasm size ${gzip_bytes} exceeds budget ${WASM_GZIP_MAX_BYTES}" >&2
  fi
}

echo "WASM Size Report"
echo
echo "Budget: raw <= $(format_int "${WASM_MAX_BYTES}") bytes, gzip <= $(format_int "${WASM_GZIP_MAX_BYTES}") bytes."
echo
echo "+----------+----------------+-----------+----------------+-----------+--------+"
printf '| %-8s | %14s | %9s | %14s | %9s | %-6s |\n' "Target" "Raw (bytes)" "Raw KiB" "Gzip (bytes)" "Gzip KiB" "Status"
echo "+----------+----------------+-----------+----------------+-----------+--------+"

if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    echo "## WASM Size Report"
    echo
    echo "Budget: raw <= $(format_int "${WASM_MAX_BYTES}") bytes, gzip <= $(format_int "${WASM_GZIP_MAX_BYTES}") bytes."
    echo
    echo "| Target | Raw (bytes) | Raw (KiB) | Gzip (bytes) | Gzip (KiB) | Status |"
    echo "| --- | ---: | ---: | ---: | ---: | --- |"
  } >> "$GITHUB_STEP_SUMMARY"
fi

report_target "web" "$web_wasm"
report_target "bundler" "$bundler_wasm"
echo "+----------+----------------+-----------+----------------+-----------+--------+"

echo
if (( overall_failed )); then
  echo "WASM size budget check failed." >&2
  exit 1
fi

echo "WASM size budget check passed."
