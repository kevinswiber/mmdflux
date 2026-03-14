#!/usr/bin/env bash
# Compare mmdflux ASCII output with dagre.js order layers for fixtures.
#
# Usage:
#   ./scripts/compare-dagre.sh              # all fixtures
#   ./scripts/compare-dagre.sh foo bar      # specific fixtures by name
#
# Output goes to /tmp/mmdflux-dagre-compare/
# Each fixture gets:
#   <name>.mmdflux.txt        (mmdflux ASCII output)
#   <name>.dagre.json         (dagre input JSON)
#   <name>.dagre.order.txt    (dagre order/rank dump)
#   <name>.mmd                (fixture source)
#   index.html                (side-by-side viewer)

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURES="$REPO/tests/fixtures/flowchart"
OUTDIR="/tmp/mmdflux-dagre-compare"
MMDFLUX="$REPO/target/debug/mmdflux"
DAGRE_INPUT_JQ="$REPO/scripts/mmds-to-dagre-input.jq"
DAGRE_ORDER_JS="$REPO/scripts/dump-dagre-order.js"

mkdir -p "$OUTDIR"

# Build binary if needed
if [[ ! -x "$MMDFLUX" ]]; then
  echo "Building mmdflux..."
  cargo build --quiet --manifest-path "$REPO/Cargo.toml" --bin mmdflux
fi

# Collect fixture list
if [[ $# -gt 0 ]]; then
  files=()
  for name in "$@"; do
    f="$FIXTURES/${name}.mmd"
    if [[ -f "$f" ]]; then
      files+=("$f")
    else
      echo "Warning: fixture not found: $f" >&2
    fi
  done
else
  files=("$FIXTURES"/*.mmd)
fi

if [[ ! -f "$DAGRE_ORDER_JS" ]]; then
  echo "Missing $DAGRE_ORDER_JS" >&2
  exit 1
fi

if [[ ${#files[@]} -eq 0 ]]; then
  echo "No fixtures found." >&2
  exit 1
fi

echo "Comparing ${#files[@]} fixtures..."
echo "Output: $OUTDIR"

# Generate outputs
for f in "${files[@]}"; do
  name="$(basename "$f" .mmd)"
  echo -n "  $name ... "

  # mmdflux ASCII output
  "$MMDFLUX" "$f" > "$OUTDIR/${name}.mmdflux.txt" 2>/dev/null || true

  # dagre input JSON + order dump
  "$MMDFLUX" "$f" --format mmds 2>/dev/null | jq -f "$DAGRE_INPUT_JQ" > "$OUTDIR/${name}.dagre.json" 2>/dev/null || true
  node "$DAGRE_ORDER_JS" "$OUTDIR/${name}.dagre.json" > "$OUTDIR/${name}.dagre.order.txt" 2>/dev/null || true

  # copy source for reference
  cp "$f" "$OUTDIR/${name}.mmd"

  echo "done"
done

# Generate HTML comparison page
cat > "$OUTDIR/index.html" <<'HEADER'
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>mmdflux vs dagre.js Comparison</title>
<style>
  body { font-family: system-ui, sans-serif; margin: 20px; background: #f5f5f5; }
  h1 { margin-bottom: 8px; }
  .subtitle { color: #666; margin-bottom: 24px; }
  .fixture {
    background: white; border: 1px solid #ddd; border-radius: 8px;
    margin-bottom: 24px; padding: 16px;
  }
  .fixture h2 { margin: 0 0 4px 0; font-size: 18px; cursor: pointer; }
  .fixture .filename { color: #888; font-size: 13px; margin-bottom: 12px; }
  .compare { display: flex; gap: 24px; align-items: flex-start; }
  .panel { flex: 1; min-width: 0; }
  .panel h3 { margin: 0 0 8px 0; font-size: 14px; color: #555; }
  pre {
    background: #1e1e1e; color: #d4d4d4; padding: 12px; border-radius: 4px;
    overflow-x: auto; font-size: 13px; line-height: 1.4;
    white-space: pre; font-family: 'SF Mono', 'Menlo', 'Monaco', monospace;
  }
  .dagre pre { background: #111827; }
  .source { margin-top: 8px; }
  .source summary {
    cursor: pointer; font-size: 13px; color: #888; user-select: none;
  }
  .source pre { background: #f8f8f8; color: #333; font-size: 12px; }
</style>
</head>
<body>
<h1>mmdflux vs dagre.js Comparison</h1>
HEADER

echo "<p class=\"subtitle\">Generated: $(date '+%Y-%m-%d %H:%M:%S') — ${#files[@]} fixtures</p>" >> "$OUTDIR/index.html"

for f in "${files[@]}"; do
  name="$(basename "$f" .mmd)"
  mmdflux_file="$OUTDIR/${name}.mmdflux.txt"
  dagre_order_file="$OUTDIR/${name}.dagre.order.txt"
  src_file="$OUTDIR/${name}.mmd"

  if [[ -f "$mmdflux_file" ]]; then
    mmdflux_output="$(sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g' "$mmdflux_file")"
  else
    mmdflux_output="(no output)"
  fi

  if [[ -f "$dagre_order_file" ]]; then
    dagre_output="$(sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g' "$dagre_order_file")"
  else
    dagre_output="(no output)"
  fi

  if [[ -f "$src_file" ]]; then
    mmd_source="$(sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g' "$src_file")"
  else
    mmd_source="(missing source)"
  fi

  cat >> "$OUTDIR/index.html" <<FIXTURE
<div class="fixture">
  <h2>$name</h2>
  <div class="filename">tests/fixtures/flowchart/${name}.mmd</div>
  <div class="compare">
    <div class="panel">
      <h3>mmdflux (ASCII)</h3>
      <pre>${mmdflux_output}</pre>
    </div>
    <div class="panel dagre">
      <h3>dagre.js (order/rank dump)</h3>
      <pre>${dagre_output}</pre>
    </div>
  </div>
  <details class="source">
    <summary>Show source (.mmd)</summary>
    <pre>${mmd_source}</pre>
  </details>
</div>
FIXTURE

done

cat >> "$OUTDIR/index.html" <<'FOOTER'
</body>
</html>
FOOTER

echo ""
echo "Done! Open the comparison page:"
echo "  open $OUTDIR/index.html"
