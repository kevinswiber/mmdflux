#!/bin/bash
set -euo pipefail

# Get repo root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Dependencies
DAGRE_ROOT="${DAGRE_ROOT:-$REPO_ROOT/deps/dagre}"
MERMAID_ROOT="${MERMAID_ROOT:-$REPO_ROOT/deps/mermaid}"

# Check dependencies
if [ ! -d "$DAGRE_ROOT" ]; then
  echo "Error: Dagre not found at $DAGRE_ROOT"
  echo "Run ./scripts/setup-debug-deps.sh first."
  exit 1
fi

if [ ! -d "$MERMAID_ROOT" ]; then
  echo "Error: Mermaid not found at $MERMAID_ROOT"
  echo "Run ./scripts/setup-debug-deps.sh first."
  exit 1
fi

# Build mmdflux binaries
echo "Building mmdflux..."
cargo build --manifest-path "$REPO_ROOT/Cargo.toml"

MMDFLUX="$REPO_ROOT/target/debug/mmdflux"
DAGRE_INPUT_JQ="$REPO_ROOT/scripts/mmds-to-dagre-input.jq"

# Fixtures to process
FIXTURES=(
  backward_in_subgraph
  external_node_subgraph
  multi_subgraph
  nested_subgraph
  nested_subgraph_only
  simple_subgraph
  subgraph_edges
)

PARITY_DIR="$REPO_ROOT/tests/parity-fixtures"
FIXTURE_DIR="$REPO_ROOT/tests/fixtures/flowchart"

for fixture in "${FIXTURES[@]}"; do
  echo "Processing $fixture..."

  outdir="$PARITY_DIR/$fixture"
  mkdir -p "$outdir"
  input="$FIXTURE_DIR/${fixture}.mmd"

  if [ ! -f "$input" ]; then
    echo "  Warning: $input not found, skipping"
    continue
  fi

  # 1. Extract dagre input from mmdflux (render to MMDS, transform with jq)
  echo "  Extracting dagre input..."
  "$MMDFLUX" "$input" --format mmds 2>"$outdir/mmdflux-dagre-input.stderr" | jq -f "$DAGRE_INPUT_JQ" > "$outdir/mmdflux-dagre-input.json" 2>>"$outdir/mmdflux-dagre-input.stderr" || true

  # 2. Run dagre.js to get expected layout
  echo "  Running dagre.js layout..."
  DAGRE_ROOT="$DAGRE_ROOT" node "$SCRIPT_DIR/dump-dagre-layout.js" "$outdir/mmdflux-dagre-input.json" > "$outdir/dagre-layout.json" 2>"$outdir/dagre-layout.stderr" || true

  # 3. Get dagre border nodes
  echo "  Extracting dagre border nodes..."
  MMDFLUX_DAGRE_SKIP_TRANSLATE=1 DAGRE_ROOT="$DAGRE_ROOT" node "$SCRIPT_DIR/dump-dagre-borders.js" "$outdir/mmdflux-dagre-input.json" > "$outdir/dagre-border-nodes.txt" 2>"$outdir/dagre-border-nodes.stderr" || true

  # 4. Get mmdflux border nodes
  echo "  Extracting mmdflux border nodes..."
  MMDFLUX_DEBUG_BORDER_NODES=1 "$MMDFLUX" "$input" > /dev/null 2>"$outdir/mmdflux-border-nodes.txt" || true

  echo "  Done: $fixture"
done

echo ""
echo "Fixtures refreshed in $PARITY_DIR"
echo ""
echo "Run 'cargo test --test dagre_parity' to verify parity."
