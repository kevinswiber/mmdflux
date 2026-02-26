#!/usr/bin/env bash
# Refresh README showcase assets from a Mermaid source diagram.
#
# Usage:
#   ./scripts/refresh-readme-assets.sh
#   ./scripts/refresh-readme-assets.sh --source path/to/demo.mmd --name demo
#   ./scripts/refresh-readme-assets.sh --mmdflux-bin ./target/release/mmdflux

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"

SOURCE="docs/assets/readme/at-a-glance.mmd"
OUT_DIR="docs/assets/readme"
NAME="at-a-glance"
MMDFLUX_BIN="${MMDFLUX_BIN:-}"

LAYOUT_ENGINE="flux-layered"
EDGE_PRESET="curved-step"
GEOMETRY_LEVEL="routed"
PATH_DETAIL="compact"

usage() {
  cat <<'EOF'
Refresh README showcase assets from a Mermaid source diagram.

Usage:
  ./scripts/refresh-readme-assets.sh
  ./scripts/refresh-readme-assets.sh --source docs/assets/readme/at-a-glance.mmd
  ./scripts/refresh-readme-assets.sh --source examples/demo.mmd --name demo
  ./scripts/refresh-readme-assets.sh --out-dir docs/assets/readme --name at-a-glance
  ./scripts/refresh-readme-assets.sh --mmdflux-bin ./target/release/mmdflux

Options:
  -s, --source <path>       Mermaid input file (.mmd)
  -o, --out-dir <path>      Output directory (default: docs/assets/readme)
  -n, --name <name>         Output basename (default: at-a-glance)
      --mmdflux-bin <path>  Use a prebuilt mmdflux binary instead of cargo run
  -h, --help                Show this help

Environment:
  MMDFLUX_BIN               Same as --mmdflux-bin
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -s|--source)
      SOURCE="$2"
      shift 2
      ;;
    -o|--out-dir)
      OUT_DIR="$2"
      shift 2
      ;;
    -n|--name)
      NAME="$2"
      shift 2
      ;;
    --mmdflux-bin)
      MMDFLUX_BIN="$2"
      shift 2
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

resolve_path() {
  local path="$1"
  if [[ "$path" = /* ]]; then
    printf '%s\n' "$path"
  else
    printf '%s\n' "$REPO/$path"
  fi
}

SOURCE="$(resolve_path "$SOURCE")"
OUT_DIR="$(resolve_path "$OUT_DIR")"
mkdir -p "$OUT_DIR"

if [[ ! -f "$SOURCE" ]]; then
  echo "Source file not found: $SOURCE" >&2
  exit 1
fi

if [[ -n "$MMDFLUX_BIN" ]]; then
  MMDFLUX_BIN="$(resolve_path "$MMDFLUX_BIN")"
  if [[ ! -x "$MMDFLUX_BIN" ]]; then
    echo "mmdflux binary is not executable: $MMDFLUX_BIN" >&2
    exit 1
  fi
fi

MMD_OUT="$OUT_DIR/$NAME.mmd"
TEXT_OUT="$OUT_DIR/$NAME.txt"
SVG_OUT="$OUT_DIR/$NAME.svg"
SVG_LIGHT_OUT="$OUT_DIR/$NAME-light.svg"
SVG_DARK_OUT="$OUT_DIR/$NAME-dark.svg"
MMDS_OUT="$OUT_DIR/$NAME.mmds.json"

run_mmdflux() {
  if [[ -n "$MMDFLUX_BIN" ]]; then
    "$MMDFLUX_BIN" "$@"
  else
    (cd "$REPO" && cargo run --quiet -- "$@")
  fi
}

echo "Refreshing README assets:"
echo "  source: $SOURCE"
echo "  output: $OUT_DIR"
echo

if [[ "$SOURCE" != "$MMD_OUT" ]]; then
  cp "$SOURCE" "$MMD_OUT"
fi

run_mmdflux --format text "$SOURCE" > "$TEXT_OUT"
run_mmdflux --format mmds --layout-engine "$LAYOUT_ENGINE" --geometry-level "$GEOMETRY_LEVEL" --path-detail "$PATH_DETAIL" "$SOURCE" > "$MMDS_OUT"

tmp_svg_raw="$(mktemp)"
run_mmdflux --format svg --layout-engine "$LAYOUT_ENGINE" --edge-preset "$EDGE_PRESET" "$SOURCE" -o "$tmp_svg_raw"

# Light-mode variant.
tmp_svg_light="$(mktemp)"
sed -e 's/background-color: transparent;/background-color: #ffffff;/g' "$tmp_svg_raw" > "$tmp_svg_light"
mv "$tmp_svg_light" "$SVG_LIGHT_OUT"

# Dark-mode variant tuned for GitHub dark themes.
tmp_svg_dark="$(mktemp)"
sed \
  -e 's/background-color: transparent;/background-color: #0d1117;/g' \
  -e 's/background-color: #ffffff;/background-color: #0d1117;/g' \
  -e 's/fill="white"/fill="#161b22"/g' \
  -e 's/fill="#333"/fill="#e6edf3"/g' \
  -e 's/stroke="#333"/stroke="#8b949e"/g' \
  "$tmp_svg_raw" > "$tmp_svg_dark"
mv "$tmp_svg_dark" "$SVG_DARK_OUT"

# Keep historical path stable: `<name>.svg` is the light variant.
cp "$SVG_LIGHT_OUT" "$SVG_OUT"
rm -f "$tmp_svg_raw"
chmod 644 "$SVG_OUT" "$SVG_LIGHT_OUT" "$SVG_DARK_OUT"

echo "Wrote:"
ls -lh "$MMD_OUT" "$TEXT_OUT" "$SVG_OUT" "$SVG_LIGHT_OUT" "$SVG_DARK_OUT" "$MMDS_OUT"
