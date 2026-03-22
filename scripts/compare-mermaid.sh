#!/usr/bin/env bash
# Compare mmdflux ASCII output with Mermaid (mmdc) SVG output for all fixtures.
#
# Usage:
#   ./scripts/compare-mermaid.sh              # all fixtures
#   ./scripts/compare-mermaid.sh double_skip  # single fixture by name
#   ./scripts/compare-mermaid.sh --open       # all fixtures, open in browser
#
# Output goes to /tmp/mmdflux-compare/
# Each fixture gets:
#   <name>.txt (mmdflux text)
#   <name>.mmdflux.svg (mmdflux svg)
#   <name>.mermaid.svg (mermaid svg)
# An index.html is generated for easy side-by-side viewing.

set -euo pipefail

auto_open=false
args=()
for arg in "$@"; do
    if [[ "$arg" == "--open" ]]; then
        auto_open=true
    else
        args+=("$arg")
    fi
done
set -- "${args[@]+"${args[@]}"}"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURES="$REPO/tests/fixtures/flowchart"
OUTDIR="/tmp/mmdflux-compare"
MMDFLUX="$REPO/target/debug/mmdflux"

mkdir -p "$OUTDIR"

cargo build -q --manifest-path "$REPO/Cargo.toml"

# --- Cache setup ---
CACHEFILE="$OUTDIR/cache.json"
CACHE_FLAT=$(mktemp)

# Parse existing cache into flat key=value format for easy lookup
if [[ -f "$CACHEFILE" ]]; then
    jq -r '"binary_hash=\(.binary_hash // "")",
           (.fixtures // {} | to_entries[] | "fixture:\(.key)=\(.value)")' \
        "$CACHEFILE" > "$CACHE_FLAT" 2>/dev/null || true
fi

cached_value() {
    grep "^${1}=" "$CACHE_FLAT" 2>/dev/null | head -1 | cut -d= -f2- || true
}

# Compute binary hash
binary_hash=$(shasum -a 256 "$MMDFLUX" | cut -d' ' -f1)
cached_binary=$(cached_value "binary_hash")
binary_changed=true
if [[ -n "$cached_binary" ]] && [[ "$binary_hash" == "$cached_binary" ]]; then
    binary_changed=false
fi

# Track fixture hashes for cache update (temp file, one "name=hash" per line)
NEW_HASHES=$(mktemp)
cleanup() { rm -f "$CACHE_FLAT" "$NEW_HASHES"; }
trap cleanup EXIT
skipped=0
generated=0

# Collect fixture list
if [[ $# -gt 0 ]]; then
    # Filter to requested fixtures
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

echo "Comparing ${#files[@]} fixtures..."
echo "Output: $OUTDIR"
if $binary_changed; then
    echo "Binary: changed"
else
    echo "Binary: unchanged"
fi
echo ""

# Generate outputs
for f in "${files[@]}"; do
    name="$(basename "$f" .mmd)"
    echo -n "  $name ... "

    syntax_hash=$(shasum -a 256 "$f" | cut -d' ' -f1)
    echo "fixture:$name=$syntax_hash" >> "$NEW_HASHES"
    cached_syntax=$(cached_value "fixture:$name")

    syntax_changed=true
    if [[ -n "$cached_syntax" ]] && [[ "$syntax_hash" == "$cached_syntax" ]]; then
        syntax_changed=false
    fi

    # Skip mmdflux if both syntax and binary are unchanged and outputs exist
    skip_mmdflux=false
    if ! $syntax_changed && ! $binary_changed; then
        if [[ -f "$OUTDIR/${name}.txt" && -f "$OUTDIR/${name}.mmdflux.svg" ]]; then
            skip_mmdflux=true
        fi
    fi

    # Skip mmdc if syntax is unchanged and output exists
    skip_mmdc=false
    if ! $syntax_changed; then
        if [[ -f "$OUTDIR/${name}.mermaid.svg" ]]; then
            skip_mmdc=true
        fi
    fi

    if $skip_mmdflux && $skip_mmdc; then
        echo "cached"
        skipped=$((skipped + 1))
        continue
    fi

    generated=$((generated + 1))
    parts=()

    if ! $skip_mmdflux; then
        # mmdflux text output (unicode)
        "$MMDFLUX" "$f" > "$OUTDIR/${name}.txt" 2>/dev/null || true
        # mmdflux SVG output
        "$MMDFLUX" --format svg "$f" > "$OUTDIR/${name}.mmdflux.svg" 2>/dev/null || true
    else
        parts+=("mmdflux cached")
    fi

    if ! $skip_mmdc; then
        # Mermaid SVG output
        mmdc -i "$f" -o "$OUTDIR/${name}.mermaid.svg" -b transparent --quiet 2>/dev/null || {
            echo "mmdc failed"
            continue
        }
    else
        parts+=("mmdc cached")
    fi

    if [[ ${#parts[@]} -gt 0 ]]; then
        # nosemgrep: bash.lang.security.ifs-tampering.ifs-tampering
        echo "$(IFS=', '; echo "${parts[*]}"), done"
    else
        echo "done"
    fi
done

# Write updated cache (merge with existing entries for fixtures not in this run)
new_fixtures=$(jq -Rn \
    '[inputs | capture("^fixture:(?<k>[^=]+)=(?<v>.+)$") | {key: .k, value: .v}] | from_entries' \
    "$NEW_HASHES")

if [[ -f "$CACHEFILE" ]]; then
    jq -S --arg bh "$binary_hash" --argjson nf "$new_fixtures" \
        '.binary_hash = $bh | .fixtures = ((.fixtures // {}) + $nf)' \
        "$CACHEFILE" > "$CACHEFILE.tmp" && mv "$CACHEFILE.tmp" "$CACHEFILE"
else
    jq -Sn --arg bh "$binary_hash" --argjson nf "$new_fixtures" \
        '{binary_hash: $bh, fixtures: $nf}' > "$CACHEFILE"
fi

echo ""
echo "Generated: $generated, Cached: $skipped"

# Generate HTML comparison page
cat > "$OUTDIR/index.html" <<'HEADER'
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>mmdflux vs Mermaid Comparison</title>
<style>
  body { font-family: system-ui, sans-serif; margin: 20px; background: #f5f5f5; }
  h1 { margin-bottom: 8px; }
  .subtitle { color: #666; margin-bottom: 24px; }
  .fixture {
    background: white; border: 1px solid #ddd; border-radius: 8px;
    margin-bottom: 24px; padding: 16px;
  }
  .fixture h2 {
    margin: 0 0 4px 0; font-size: 18px;
    cursor: pointer;
  }
  .fixture .filename { color: #888; font-size: 13px; margin-bottom: 12px; }
  .compare {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 16px;
    align-items: start;
  }
  .panel { min-width: 0; }
  .panel h3 { margin: 0 0 8px 0; font-size: 14px; color: #555; }
  pre {
    background: #1e1e1e; color: #d4d4d4; padding: 12px; border-radius: 4px;
    overflow: auto; font-size: 13px; line-height: 1.4; max-height: 360px;
    white-space: pre; font-family: 'SF Mono', 'Menlo', 'Monaco', monospace;
  }
  .mermaid-svg {
    border: 1px solid #eee; border-radius: 4px; padding: 8px;
    background: white; text-align: center;
  }
  .mermaid-svg img { max-width: 100%; height: auto; }
  .source pre { background: #f8f8f8; color: #333; font-size: 12px; max-height: 360px; }
</style>
</head>
<body>
<h1>mmdflux vs Mermaid Comparison</h1>
HEADER

echo "<p class=\"subtitle\">Generated: $(date '+%Y-%m-%d %H:%M:%S') &mdash; ${#files[@]} fixtures</p>" >> "$OUTDIR/index.html"

for f in "${files[@]}"; do
    name="$(basename "$f" .mmd)"
    txt_file="$OUTDIR/${name}.txt"
    mmdflux_svg_file="$OUTDIR/${name}.mmdflux.svg"
    mermaid_svg_file="$OUTDIR/${name}.mermaid.svg"

    # Read mmdflux output (HTML-escape it)
    if [[ -f "$txt_file" ]]; then
        ascii_output="$(sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g' "$txt_file")"
    else
        ascii_output="(no output)"
    fi

    # Read mermaid source
    mmd_source="$(sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g' "$f")"

    cat >> "$OUTDIR/index.html" <<FIXTURE
<div class="fixture">
  <h2>$name</h2>
  <div class="filename">tests/fixtures/flowchart/${name}.mmd</div>
  <div class="compare">
    <div class="panel">
      <h3>mmdflux (Text)</h3>
      <pre>${ascii_output}</pre>
    </div>
    <div class="panel">
      <h3>mmdflux (SVG)</h3>
      <div class="mermaid-svg">
        <img src="${name}.mmdflux.svg" alt="${name} mmdflux svg output">
      </div>
    </div>
    <div class="panel">
      <h3>Mermaid (SVG)</h3>
      <div class="mermaid-svg">
        <img src="${name}.mermaid.svg" alt="${name} mermaid output">
      </div>
    </div>
    <div class="panel source">
      <h3>Mermaid Source</h3>
      <pre>${mmd_source}</pre>
    </div>
  </div>
</div>
FIXTURE
done

cat >> "$OUTDIR/index.html" <<'FOOTER'
</body>
</html>
FOOTER

echo ""
if $auto_open; then
    echo "Done! Opening comparison page..."
    open "$OUTDIR/index.html"
else
    echo "Done! Open the comparison page:"
    echo "  open $OUTDIR/index.html"
fi
