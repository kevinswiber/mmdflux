# WebAssembly Compilation Analysis for mmdflux

This document provides a deep analysis of what would be needed to compile mmdflux to WebAssembly, with recommendations for minimizing module size while maximizing performance benefits.

## Executive Summary

**Recommended Approach**: Compile only the **dagre layout engine** to WebAssembly, implementing the parser and renderer in TypeScript/JavaScript.

| Component | Recommendation | Rationale |
|-----------|---------------|-----------|
| **Dagre Layout** | WASM | Computationally intensive (O(n²) algorithms), benefits greatly from Wasm |
| **Parser** | TypeScript | String-heavy, pest adds significant code size, JS parsers are efficient |
| **Renderer** | TypeScript | Simple grid operations, DOM/canvas integration is JS-native |
| **Graph Builder** | TypeScript | Simple data transformation, no computational bottleneck |

**Estimated Wasm module size**: 100-200 KB (gzipped: 30-60 KB) for layout-only approach.

---

## Current Architecture

```
Mermaid Text → Parser (pest) → AST → Graph Builder → Diagram → Dagre Layout → Render → Text
                  ↑                                      ↑            ↑          ↑
              ~1,700 LOC                              ~1,000 LOC  ~13,000 LOC  ~6,000 LOC
```

### Dependency Analysis

Current release binary: **2.6 MB** (stripped native)

| Dependency | Purpose | Wasm Impact |
|------------|---------|-------------|
| `clap` | CLI parsing | **Remove** - Not needed for library |
| `pest`/`pest_derive` | PEG parser generator | **Remove** - Adds ~200-400 KB to Wasm |
| `petgraph` | Graph algorithms | **Remove** - Only dead code uses it (`to_petgraph()`) |
| `thiserror` | Error types | **Keep** - Minimal overhead |

---

## Component-by-Component Analysis

### 1. Parser (`src/parser/`) - **Implement in TypeScript**

**Current Implementation**:
- Uses pest PEG grammar (`grammar.pest` - 170 lines)
- Generated parser code from `pest_derive`
- Produces AST types: `Vertex`, `EdgeSpec`, `ConnectorSpec`, `Statement`

**Why TypeScript is better**:

1. **Code Size**: pest generates substantial parsing code. The grammar alone adds 200-400 KB to the Wasm binary.

2. **String Processing**: JavaScript's native string APIs are highly optimized. Wasm string handling requires:
   - UTF-8 encoding/decoding at boundaries
   - Memory management for strings
   - Extra marshaling overhead

3. **Existing Parsers**: Several mature Mermaid parsers exist:
   - `mermaid-js/mermaid` itself has a JavaScript parser
   - Parser combinator libraries (Chevrotain, Nearley) work well in JS

4. **Grammar Complexity**: The flowchart grammar is relatively simple:
   - Header parsing (`graph TD`, `flowchart LR`)
   - Node shapes (12 variants)
   - Edge connectors (solid, dotted, thick with arrows)
   - Subgraph blocks

**TypeScript Parser Approach**:
```typescript
interface Vertex {
  id: string;
  shape: 'rect' | 'round' | 'diamond' | /* ... */;
  text?: string;
}

interface Edge {
  from: string;
  to: string;
  stroke: 'solid' | 'dotted' | 'thick';
  label?: string;
  arrow: { left: boolean; right: boolean };
}

function parseFlowchart(input: string): {
  direction: 'TD' | 'BT' | 'LR' | 'RL';
  nodes: Map<string, Vertex>;
  edges: Edge[];
}
```

---

### 2. Dagre Layout Engine (`src/dagre/`) - **Compile to WASM**

**Current Implementation**: ~13,000 lines implementing the Sugiyama framework:

| Phase | File(s) | Lines | Complexity |
|-------|---------|-------|------------|
| Cycle removal | `acyclic.rs` | 150 | O(V + E) |
| Rank assignment | `rank.rs`, `network_simplex.rs` | 1,300 | O(V·E) network simplex |
| Normalization | `normalize.rs` | 711 | O(E) |
| Crossing reduction | `order.rs` | 2,254 | O(k·V²) barycenter sweeps |
| Coordinate assignment | `bk.rs`, `position.rs` | 3,100 | O(V²) Brandes-Köpf |
| Compound graphs | `nesting.rs`, `border.rs` | 1,900 | O(V) |

**Why Wasm is better**:

1. **Computational Intensity**: The crossing reduction and coordinate assignment phases involve:
   - Nested loops over ranks and nodes
   - Sorting operations
   - Floating-point arithmetic
   - HashMap lookups

2. **Data Structure Density**: Layout uses:
   - Dense vectors indexed by node ID
   - Sparse adjacency via HashMap
   - No string processing after initial construction

3. **Predictable Memory Access**: Layout algorithms work on fixed-size numeric data, benefiting from Wasm's linear memory model.

4. **Performance Critical Path**: For large diagrams (50+ nodes), layout dominates execution time.

**Wasm Interface Design**:

```typescript
// Input: JSON-serializable graph description
interface LayoutInput {
  nodes: Array<{ id: string; width: number; height: number }>;
  edges: Array<{ from: string; to: string; label?: { width: number; height: number } }>;
  direction: 'TB' | 'BT' | 'LR' | 'RL';
  config: {
    nodeSep: number;
    rankSep: number;
    edgeSep: number;
    margin: number;
  };
  // Compound graph support
  parents?: Map<string, string>;
}

// Output: Positioned nodes and edge paths
interface LayoutResult {
  nodes: Map<string, { x: number; y: number; width: number; height: number }>;
  edges: Array<{
    from: string;
    to: string;
    points: Array<{ x: number; y: number }>;
  }>;
  width: number;
  height: number;
}
```

**Wasm Export Strategy**:

```rust
// Single function export for minimal interface
#[wasm_bindgen]
pub fn layout(input_json: &str) -> String {
    // Parse JSON input
    // Run dagre layout
    // Return JSON output
}
```

Using JSON serialization adds ~10-20% overhead but:
- Keeps the Wasm interface simple (single string in, string out)
- Avoids complex memory management across the boundary
- Works with any JS framework

---

### 3. Graph Builder (`src/graph/`) - **Implement in TypeScript**

**Current Implementation**: ~1,000 lines
- `Diagram` struct with nodes/edges
- Shape enumeration
- Direction handling

**Why TypeScript is better**:

1. **Simple Data Transformation**: Just maps AST to layout input format
2. **No Algorithms**: Pure data mapping, no computational work
3. **Integration Point**: Natural place to handle JS-specific concerns

---

### 4. Renderer (`src/render/`) - **Implement in TypeScript**

**Current Implementation**: ~6,000 lines
- Canvas-based text grid
- Box-drawing character selection
- Edge routing (router.rs - 1,434 lines)
- Shape rendering

**Why TypeScript is better**:

1. **Output Format**: Web apps likely want:
   - SVG for scalable vector graphics
   - Canvas 2D API for bitmaps
   - HTML/CSS for interactive diagrams

   Not ASCII text with Unicode box-drawing.

2. **DOM Integration**: Character encoding, font metrics, and display are JS-native concerns.

3. **Customization**: Web apps want theming, interactivity, custom shapes.

**Edge Router Consideration**:

The edge router (`router.rs`) computes orthogonal paths from waypoints. This could go either way:

| In Wasm | In TypeScript |
|---------|---------------|
| +Consistent with layout | +Easier customization |
| +Uses layout coordinates directly | +Access to rendered node bounds |
| -Adds ~50 KB | -Duplicates waypoint logic |

**Recommendation**: Include basic waypoint-to-path conversion in Wasm, but allow JS override for custom routing.

---

## Wasm Build Configuration

### Cargo.toml Changes

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
# Remove for wasm build:
# clap, pest, pest_derive

# Keep:
thiserror = "2"

# Add for wasm:
wasm-bindgen = "0.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[profile.release]
opt-level = "z"        # Optimize for size
lto = true             # Link-time optimization
codegen-units = 1      # Single codegen unit for better optimization
panic = "abort"        # Don't include panic unwinding
strip = true           # Strip symbols

[profile.release.package."*"]
opt-level = "z"
```

### Feature Flags

```toml
[features]
default = ["cli"]
cli = ["clap"]
wasm = ["wasm-bindgen", "serde", "serde_json"]
```

### Build Commands

```bash
# Native CLI
cargo build --release --features cli

# Wasm library
cargo build --release --target wasm32-unknown-unknown --features wasm --no-default-features

# Optimize wasm further
wasm-opt -Oz -o mmdflux_opt.wasm target/wasm32-unknown-unknown/release/mmdflux.wasm
```

---

## Size Estimates

### Current (Full Crate)
| Component | Estimated Wasm Size |
|-----------|---------------------|
| pest parser | 200-400 KB |
| dagre layout | 150-250 KB |
| render module | 100-150 KB |
| std library overhead | 100-200 KB |
| **Total** | **550-1000 KB** |

### Recommended (Layout Only)
| Component | Estimated Wasm Size |
|-----------|---------------------|
| dagre layout | 150-250 KB |
| serde_json | 50-80 KB |
| wasm-bindgen glue | 10-20 KB |
| std subset | 50-100 KB |
| **Total** | **260-450 KB** |
| **Gzipped** | **80-150 KB** |

With aggressive optimization (`wasm-opt -Oz`):
- **Optimized**: 100-200 KB
- **Gzipped**: 30-60 KB

---

## TypeScript Integration

### Package Structure

```
@mmdflux/layout
├── src/
│   ├── wasm/
│   │   └── mmdflux_bg.wasm      # Layout engine
│   ├── parser/
│   │   └── flowchart.ts         # Mermaid parser
│   ├── renderer/
│   │   ├── svg.ts               # SVG renderer
│   │   ├── canvas.ts            # Canvas 2D renderer
│   │   └── ascii.ts             # Text/ASCII renderer
│   └── index.ts                 # Main API
└── package.json
```

### API Design

```typescript
import { parse, layout, renderSvg } from '@mmdflux/layout';

// Full pipeline
const diagram = parse(`
  graph TD
    A[Start] --> B{Decision}
    B -->|Yes| C[OK]
    B -->|No| D[Cancel]
`);

const positioned = await layout(diagram);
const svg = renderSvg(positioned);

// Or use layout directly with pre-computed dimensions
const result = await layoutRaw({
  nodes: [
    { id: 'A', width: 80, height: 40 },
    { id: 'B', width: 100, height: 60 },
  ],
  edges: [{ from: 'A', to: 'B' }],
  direction: 'TB',
});
```

### Lazy Loading

```typescript
// Only load Wasm when needed
let layoutEngine: LayoutEngine | null = null;

async function layout(diagram: Diagram): Promise<LayoutResult> {
  if (!layoutEngine) {
    const wasm = await import('./wasm/mmdflux_bg.wasm');
    layoutEngine = new LayoutEngine(wasm);
  }
  return layoutEngine.layout(diagram);
}
```

---

## Performance Comparison

Expected performance characteristics:

| Operation | JS-only | Wasm Layout |
|-----------|---------|-------------|
| Parse (100 nodes) | 2-5 ms | N/A |
| Layout (100 nodes) | 50-100 ms | 5-15 ms |
| Render SVG (100 nodes) | 5-10 ms | N/A |
| **Total** | 57-115 ms | 12-30 ms |

For small diagrams (<20 nodes), the difference is negligible. For large diagrams (100+ nodes), Wasm provides 5-10x speedup on the layout phase.

---

## Alternative Approaches Considered

### 1. Full Rust to Wasm
**Rejected**: Parser adds 200-400 KB, render is JS-native concern.

### 2. Pure TypeScript with dagre.js
**Alternative**: Use existing `dagre` npm package.
**Tradeoff**: dagre.js is unmaintained (last update 2014), has known bugs. mmdflux fixes many dagre issues.

### 3. Wasm + WASI for Node.js
**Alternative**: Use WASI for server-side rendering.
**Consideration**: Good for SSR use cases, same Wasm binary works.

### 4. AssemblyScript
**Alternative**: Write layout in AssemblyScript for smaller binary.
**Tradeoff**: Loses Rust's correctness guarantees, requires rewrite.

---

## Migration Path

### Phase 1: Wasm-Ready Refactoring
1. Remove `clap` from library code (already in `main.rs`)
2. Remove dead `petgraph` usage
3. Add `#[cfg(feature = "wasm")]` for wasm-bindgen exports
4. Create JSON-based layout interface

### Phase 2: TypeScript Package
1. Port parser to TypeScript (or use existing Mermaid parser)
2. Create renderer implementations (SVG, Canvas, ASCII)
3. Wasm bindings and lazy loading
4. Publish npm package

### Phase 3: Optimization
1. Profile and optimize hot paths
2. Consider SharedArrayBuffer for zero-copy data
3. Add Web Worker support for non-blocking layout

---

## Conclusion

The optimal Wasm strategy for mmdflux is:

1. **Compile only `src/dagre/`** to WebAssembly (~100-200 KB gzipped)
2. **Implement parser in TypeScript** - simpler, smaller, leverages existing JS ecosystem
3. **Implement renderer in TypeScript** - enables SVG/Canvas output, theming, interactivity
4. **Use JSON interface** - simple, debuggable, framework-agnostic

This approach delivers:
- **5-10x layout speedup** for large diagrams
- **Minimal bundle size** (~30-60 KB gzipped for Wasm module)
- **Full web platform integration** for parsing and rendering
- **Maintainability** by keeping complex algorithms in well-tested Rust code
