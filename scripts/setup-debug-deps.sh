#!/bin/bash
set -euo pipefail

# Get repo root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEPS_DIR="$REPO_ROOT/deps"

# Pinned versions
DAGRE_REPO="https://github.com/dagrejs/dagre.git"
DAGRE_TAG="v0.8.5"
DAGRE_SHA="f56edb1abbb8530e532158f7cbd403228f5b0018"

MERMAID_REPO="https://github.com/mermaid-js/mermaid.git"
MERMAID_SHA="09d06501c36e6b908fb19d02b441a9073a98d46c"

echo "Setting up debug dependencies in $DEPS_DIR"
mkdir -p "$DEPS_DIR"

# Clone and setup dagre
if [ -d "$DEPS_DIR/dagre" ]; then
  echo "dagre already exists, skipping clone"
else
  echo "Cloning dagre at $DAGRE_TAG ($DAGRE_SHA)..."
  git clone --depth 1 --branch "$DAGRE_TAG" "$DAGRE_REPO" "$DEPS_DIR/dagre"

  # Verify SHA
  ACTUAL_SHA=$(git -C "$DEPS_DIR/dagre" rev-parse HEAD)
  if [ "$ACTUAL_SHA" != "$DAGRE_SHA" ]; then
    echo "Warning: dagre SHA mismatch!"
    echo "  Expected: $DAGRE_SHA"
    echo "  Actual:   $ACTUAL_SHA"
  fi

  echo "Installing dagre dependencies..."
  (cd "$DEPS_DIR/dagre" && npm install)
fi

# Clone and setup mermaid
if [ -d "$DEPS_DIR/mermaid" ]; then
  echo "mermaid already exists, skipping clone"
else
  echo "Cloning mermaid at $MERMAID_SHA..."
  git clone "$MERMAID_REPO" "$DEPS_DIR/mermaid"
  git -C "$DEPS_DIR/mermaid" checkout "$MERMAID_SHA"

  echo "Installing mermaid dependencies..."
  (cd "$DEPS_DIR/mermaid" && pnpm install)

  echo "Building mermaid..."
  (cd "$DEPS_DIR/mermaid" && pnpm run build)
fi

echo ""
echo "Setup complete!"
echo ""
echo "Debug environment is ready. You can now run:"
echo "  cargo test --test dagre_parity          # Run parity tests"
echo "  ./scripts/refresh-parity-fixtures.sh  # Regenerate fixtures"
echo ""
echo "Environment variables (optional, for custom paths):"
echo "  DAGRE_ROOT=$DEPS_DIR/dagre"
echo "  MERMAID_ROOT=$DEPS_DIR/mermaid"
