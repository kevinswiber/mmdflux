# Edge Routing System — Product Requirements Document

**Status:** DRAFT — living document, updated as research and implementation progress

**Last updated:** 2026-02-17

**Owner:** Kevin Swiber

**Note:** This Product Requirements Document is included alongside the mmdflux codebase for posterity. It describes the heuristics of the Layered algorithm of the Flux engine without having to delve through the code, though the code should be considered the ultimate source of truth. This document may become out-of-sync with the implementation. It represents the efforts of a lot of research and builds on the work of many sources, for which this project is grateful. References are listed below.

---

## 1. Purpose

This document defines the product requirements for mmdflux's edge routing system across all supported output formats (text/ASCII, SVG, MMDS JSON). It consolidates research findings, architectural constraints, and design decisions into a single reference that guides implementation of edge path construction, port attachment, and visual quality across the four supported edge styles.

The routing system is the most complex subsystem in mmdflux's graph-family pipeline. It sits between layout (Sugiyama coordinate assignment) and rendering, and its quality directly determines diagram readability.

---

## 2. Scope

### In scope

- Edge path construction for graph-family diagrams (flowchart, class)
- Four edge styles: orthogonal, straight, rounded, curved
- Port attachment policy for directional layouts (TD, BT, LR, RL)
- Bend minimization and crossing avoidance heuristics
- Backward edge (cycle) routing conventions
- Label placement along routed edges
- Orthogonal routing architecture (float-first shared engine)
- MMDS JSON path serialization semantics

### Out of scope

- Sequence diagram lifeline routing (timeline family, separate pipeline)
- Layout engine internals (Sugiyama ranking, crossing reduction, coordinate assignment)
- Node shape rendering
- Force-directed or radial layout modes

---

## 3. Architecture Context

Edge routing occupies Layer 1→2 in the graph-family pipeline:

```
Diagram → Layout Engine → GraphGeometry (L1) → route_graph_geometry() → RoutedGraphGeometry (L2) → Renderer
```

The routing stage receives engine-agnostic node positions, edge topology, and layout hints (waypoints), then produces resolved polyline paths, label positions, and backward-edge markers. Both text and SVG renderers consume the same `RoutedGraphGeometry` IR.

### Current edge routings

| Mode                 | Description                                   | Status                    |
| -------------------- | --------------------------------------------- | ------------------------- |
| `full-compute`       | Build paths from layout hints + node geometry | Default                   |
| `pass-through-clip`  | Use engine-provided paths with clipping       | Used by ELK adapter       |
| `orthogonal-preview` | Float-first shared routing engine             | In hardening (plan 0076+) |

### Current edge styles (SVG)

| Style        | CLI flag                  | Description                                 |
| ------------ | ------------------------- | ------------------------------------------- |
| `curved`     | `--edge-style curved`     | Smooth B-spline through waypoints (default) |
| `straight`   | `--edge-style straight`   | Straight polyline segments                  |
| `rounded`    | `--edge-style rounded`    | Orthogonal routing with rounded corners     |
| `orthogonal` | `--edge-style orthogonal` | Axis-aligned path construction              |

Text rendering always uses orthogonal paths (inherent to the character grid).

---

## 4. Research Foundations

### 4.1 Aesthetic hierarchy for graph readability

Helen Purchase's seminal work (1997–2002) established a consistent priority order for graph readability through controlled experiments:

1. **Minimize edge crossings** — most impactful single factor
2. **Minimize bends** — second most significant
3. **Grid alignment** — users naturally prefer orthogonal layouts
4. **Symmetry and uniform edge lengths** — helpful but secondary

This hierarchy holds across both abstract graph comprehension tasks and domain-specific diagrams (UML, flowcharts). It directly informs the priority ordering of routing heuristics in mmdflux.

**Key finding:** Right-angle crossings (RAC drawings) are readable nearly as well as planar drawings, but bend count heavily affects readability. This means if two edges must cross, crossing at 90° is acceptable — but introducing unnecessary bends to avoid a crossing is often the wrong tradeoff.

### 4.2 Curved vs straight edges

Xu, Rooney, Passmore, Ham & Nguyen (2012, IEEE TVCG) conducted the most direct comparison:

- **Straight edges** produced fewer reading errors and faster task completion
- **Low-curvature Bézier curves** (Lombardi-style) performed comparably to straight edges
- **Heavy curvature** significantly degraded performance
- **Aesthetic preference vs performance gap:** users rate curved edges as more visually pleasing but perform measurably worse with them

Bar & Neta (2006) provide the psychological basis: humans have an innate preference for curved visual objects, but this preference conflicts with task performance in information-dense diagrams.

**Implication for mmdflux:** The `curved` default for SVG is defensible as a visual-quality choice that matches Mermaid.js conventions, but `orthogonal` and `straight` should be treated as the performance-optimal options for information-dense diagrams. Low curvature in `curved` mode should be preferred over heavy curvature.

### 4.3 Bend minimization

Ware et al. (2002) identified **edge continuity** — the eye's ability to follow an edge without losing track — as the core cognitive mechanism. Continuity is disrupted by:

- Direction changes (bends)
- Edge crossings along the traced path
- Branch points

Tamassia (1987) proved bend minimization for orthogonal drawings is solvable in polynomial time via minimum-cost network flow when the planar embedding is fixed. If the embedding can change, the problem becomes NP-complete.

**Bend hierarchy for readability:**

```
straight line (0 bends) > L-shape (1 bend) > Z-shape (2 bends) > staircase (3+ bends)
```

Each bend represents a direction change requiring cognitive re-orientation. However, this hierarchy is subordinate to the flow-direction convention (see §4.4).

### 4.4 Port attachment and flow direction

In hierarchical/Sugiyama layouts, the entire purpose of directional arrangement is to visually encode flow through spatial position. This creates a strong convention for port attachment:

| Direction       | Exit face (source) | Entry face (target) |
| --------------- | ------------------ | ------------------- |
| TD (top-down)   | South (bottom)     | North (top)         |
| BT (bottom-top) | North (top)        | South (bottom)      |
| LR (left-right) | East (right)       | West (left)         |
| RL (right-left) | West (left)        | East (right)        |

This convention is deeply structural to the Sugiyama framework. Edges travel between layers in the flow direction; the algorithm literally assigns nodes to layers and routes edges between adjacent layers along the flow axis.

**The ELK port model** formalizes this with "flow ports" (on sides aligned with the layout direction) and "non-flow ports" (perpendicular sides). Their `allowNonFlowPortsToSwitchSides` option only permits side switching to minimize crossings — not bends.

**Critical design principle:** Flow direction takes priority over bend minimization. A Z-shaped edge (2 bends) that exits south and enters north in a TD layout is *preferable* to an L-shaped edge (1 bend) that exits east and enters north — because the Z-shape preserves the visual flow signal while the L-shape breaks it.

**Exceptions where non-flow port attachment is correct:**

- **Same-layer edges (flat edges):** Two nodes on the same layer can't have a flow-direction edge; they route laterally via east/west faces in TD/BT layouts
- **Backward edges (cycles):** Edges against the flow direction exit/enter on the "wrong" faces — this is a useful visual signal that says "this edge goes against the flow"
- **User-specified port constraints:** Circuit schematics, dataflow diagrams, and block diagrams have semantically fixed port positions

### 4.5 Production tool implementations

**Graphviz dot:** The default hierarchical layout engine. Uses `rankdir` (TB, LR, BT, RL) to set flow direction. Edges default to flow-direction port attachment. `headport`/`tailport` attributes allow compass-point overrides. Routing uses splines by default with `splines=ortho` option for orthogonal paths.

**Eclipse ELK (KLay Layered):** The most sophisticated open-source layered layout engine. Defines port constraint levels: FREE → FIXED_SIDE → FIXED_ORDER → FIXED_POS. Supports routing styles: straight, orthogonal, splines. The layered algorithm's crossing minimization explicitly accounts for port sides during ordering.

**yFiles:** Commercial library supporting orthogonal, octilinear (45° increments), and curved (cubic Bézier) routing. Exposes `bendCost` as a tunable parameter — higher values produce straighter paths; lower values allow more routing flexibility.

**draw.io (diagrams.net):** Default orthogonal edge style with `rounded:1`. Three line styles: sharp, rounded, curved. Their guidance: *"Sharp style feels rigid but gets point across. If in doubt, stay with Sharp. Curved style friendlier to look at but requires more effort to edit."*

**Mermaid.js (via dagre):** Uses dagre for layout but pushes all edge routing downstream to the renderer. This is the root cause of mmdflux's routing complexity — dagre provides waypoints but no route construction.

---

## 5. Requirements

### 5.1 Edge path style requirements

#### R-ORTH: Orthogonal style

Orthogonal edges use axis-aligned (horizontal + vertical) segments only.

| ID        | Requirement                                                                                                                                                                                                                     | Priority | Rationale                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R-ORTH-1  | All segments must be strictly axis-aligned (horizontal or vertical)                                                                                                                                                             | P0       | Defining property of orthogonal style                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| R-ORTH-2  | Minimize total bend count per edge                                                                                                                                                                                              | P0       | Bend count is the primary readability factor after crossings (Purchase 1997)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| R-ORTH-3  | Prefer L-shape (1 bend) over Z-shape (2 bends) when geometrically possible without violating flow-direction port attachment                                                                                                     | P1       | Bend minimization within flow constraints                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| R-ORTH-4  | Z-shape (2 bends) is the standard fallback when source and target are offset on both axes                                                                                                                                       | P1       | Minimum-bend solution for offset nodes in hierarchical layout                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| R-ORTH-5  | Never produce staircase artifacts (3+ unnecessary bends) in the routing stage                                                                                                                                                   | P0       | Staircase artifacts are the most common visual quality defect                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| R-ORTH-6  | When multiple edges share a routing channel (same horizontal or vertical corridor), each edge must be assigned a distinct offset within that channel so that no two edges overlap (share identical pixel paths) for any segment | P0       | Coincident edges destroy edge continuity (Ware et al. 2002) — the eye cannot distinguish which edge is which along the shared segment. This is arguably worse than a crossing, because at a crossing the eye can follow trajectory through the intersection point; with overlap there is no trajectory information at all. The reader cannot determine how many edges exist or where each one goes without tracing from both endpoints. ELK's layered algorithm handles this explicitly by assigning each edge to a distinct routing slot within shared channels. curved styles naturally separate via divergent control points, but orthogonal (and orthogonal-rounded/straight) require explicit channel spreading. Observed in `five_fan_in.mmd` orthogonal rendering where all five edges from A–E to Target share identical horizontal segments, producing a single visible line that obscures the 5-edge fan-in structure. |
| R-ORTH-10 | Fan-in and fan-out edge groups must stagger their shared-axis segments at distinct offsets proportional to the number of edges in the group                                                                                     | P0       | Fan-in (N sources → 1 target) and fan-out (1 source → N targets) are the most common triggers for edge overlap in hierarchical layouts. In a TD fan-in, all edges converge on the same target north face; without staggering, the horizontal jog segments collapse onto the same Y-coordinate. The stagger offset between adjacent edges should be consistent (uniform spacing) and the total stagger band should be centered in the inter-rank gap to maintain visual balance. The outermost edges get the shallowest (earliest) horizontal segments; inner edges get deeper ones closer to the target. This produces a visible "funnel" shape that communicates the fan-in/fan-out structure at a glance. Observed in `five_fan_in.mmd` orthogonal rendering where the fan-in structure is invisible due to all horizontal segments overlapping.                                                                               |
| R-ORTH-7  | Terminal approach segments (the last segment entering a node) must be long enough to support arrowhead rendering                                                                                                                | P0       | Arrow direction readability                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| R-ORTH-8  | Post-routing compaction must not collapse terminal approach segments                                                                                                                                                            | P0       | Prevents arrow direction ambiguity (plan 0076 finding)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| R-ORTH-9  | Departure segments (the first segment leaving a source node) must have a minimum length in the flow direction sufficient to establish visual trajectory before any lateral jog                                                  | P0       | Without a departure stem, the edge reads as a lateral relationship rather than a hierarchical one, disrupting flow-direction comprehension at the worst possible point — the origin where the eye hasn't yet established a tracking trajectory (Ware et al. 2002 continuity principle). Observed as a orthogonal-preview regression in `label_spacing.mmd` where FULL-COMPUTE produces a visible departure stem but ORTHOGONAL-PREVIEW does not.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |

#### R-STR: Straight style

Straight edges use straight-line segments (polyline) without axis-alignment constraint. Straight style follows a two-phase pipeline: orthogonal routing for correctness, then diagonal simplification for visual identity (see DD-4).

| ID      | Requirement                                                                                                                                                 | Priority | Rationale                                                                                                                                                                                                                                                                   |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R-STR-1 | Prefer straight diagonal lines (0 bends) as the overwhelming default                                                                                        | P0       | Straight lines are optimal for readability (Xu et al. 2012)                                                                                                                                                                                                                 |
| R-STR-2 | Route orthogonally first, then simplify to diagonals as a post-processing step                                                                              | P0       | Ensures correct flow-direction port attachment and collision avoidance before visual simplification (DD-4). Observed in `diamond_fan.mmd`: raw-waypoint diagonals produce poor flow encoding; unsimplified orthogonal produces no visual distinction from orthogonal style. |
| R-STR-3 | Reject diagonal simplification when the resulting line would cross a node body; fall back to the orthogonal path for that edge                              | P0       | Collision avoidance is the top-priority constraint (DD-1)                                                                                                                                                                                                                   |
| R-STR-4 | Reject diagonal simplification when the resulting departure angle is too shallow relative to the flow direction (e.g., near-horizontal exit in a TD layout) | P1       | A near-horizontal departure in a TD layout breaks flow-direction encoding even though it reduces bend count                                                                                                                                                                 |
| R-STR-5 | Edge-to-node clipping must be precise for non-rectangular shapes                                                                                            | P1       | Diagonal lines must clip cleanly at shape boundaries                                                                                                                                                                                                                        |
| R-STR-6 | Label anchors must be revalidated after diagonal simplification                                                                                             | P1       | Simplification can remove the segment a label was anchored to (R-LABEL-2 applies)                                                                                                                                                                                           |

#### R-RND: Rounded (orthogonal-rounded) style

Rounded edges are orthogonal-routed paths with corner radii applied at bends.

| ID      | Requirement                                                   | Priority | Rationale                                              |
| ------- | ------------------------------------------------------------- | -------- | ------------------------------------------------------ |
| R-RND-1 | Underlying path construction follows orthogonal routing rules | P0       | Rounding is a visual treatment, not a routing strategy |
| R-RND-2 | Corner radius is consistent across all bends in a single edge | P1       | Visual consistency                                     |
| R-RND-3 | Corner radius adapts to avoid distortion on short segments    | P1       | Radius > segment length produces visual artifacts      |
| R-RND-4 | Corner radius is user-configurable via `--edge-radius`        | P1       | Currently implemented                                  |

#### R-CRV: Curved style

Curved edges use smooth B-spline interpolation through waypoints.

| ID      | Requirement                                                                           | Priority | Rationale                                                  |
| ------- | ------------------------------------------------------------------------------------- | -------- | ---------------------------------------------------------- |
| R-CRV-1 | Curves should maintain low curvature (control points close to the straight-line path) | P1       | Heavy curvature degrades task performance (Xu et al. 2012) |
| R-CRV-2 | Curve must pass through or near all waypoints                                         | P0       | Waypoints encode layout structure                          |
| R-CRV-3 | Curves must not cross node bodies                                                     | P1       | Collision avoidance                                        |

### 5.2 Port attachment requirements

| ID       | Requirement                                                                                                                                                                                                                                                                     | Priority | Rationale                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| R-PORT-1 | Forward edges exit the downstream face and enter the upstream face per the diagram direction                                                                                                                                                                                    | P0       | Core convention of hierarchical layout (§4.4)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| R-PORT-2 | Backward edges (cycles) exit/enter on reversed faces as a visual signal of counter-flow                                                                                                                                                                                         | P0       | Established convention across all major tools                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| R-PORT-3 | Same-layer edges route via non-flow faces (east/west in TD/BT, north/south in LR/RL)                                                                                                                                                                                            | P1       | Only viable routing for same-layer connections                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| R-PORT-4 | Attachment points spread evenly along a face when multiple edges share it                                                                                                                                                                                                       | P1       | Prevents edge overlap at nodes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| R-PORT-5 | Face capacity overflow spills to adjacent faces rather than stacking beyond readable density                                                                                                                                                                                    | P2       | Research finding from plan 0048 — not yet validated with empirical thresholds                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| R-PORT-6 | Port attachment policy must be deterministic for identical input                                                                                                                                                                                                                | P0       | MMDS serialization stability, test reproducibility                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| R-PORT-7 | Ports on any face must be inset from the node's corners by a minimum margin sufficient for the departure/terminal stem to be visually distinct from the adjacent perpendicular border line                                                                                      | P0       | When a port is placed at the extreme edge of a face, the departure or terminal stem coincides with the node's perpendicular border line and becomes perceptually invisible — the viewer cannot distinguish "edge leaving node" from "node border." This is a port allocation constraint, not a routing constraint: the router produces correct stems, but the port distributor spaces ports to the full width of the face. Observed in fan-out (Source → Target A/C) and fan-in (Source A/C → Target) fixtures where outer ports align with the left/right box edges.                                                                                                                                                                                                                                                                                                                                        |
| R-PORT-8 | When source and target nodes are near-aligned on the cross-axis (horizontal offset ≤ half the narrower node's face width in TD/BT; vertical offset ≤ half in LR/RL), the port attachment point should shift to eliminate the Z-jog, producing a straight-line edge with 0 bends | P1       | Schulze et al (2013) Figure 4b "local adjustment" strategy: port positions shift to align with the connected edge's ideal straight-line path, eliminating bends that result from even port distribution on near-aligned nodes. Gansner et al (1993) treat this as a layout-level concern (`minedge` heuristic), but when the layered layout produces residual cross-axis misalignment that falls within the face width, a port-level adjustment is the correct fallback. Purchase (1997) ranks bends as the #2 readability factor — each Z-jog costs 2 bends of unnecessary cognitive re-orientation. The threshold (half the narrower face width) ensures the port remains visually within the node boundary; offsets beyond this produce a legitimate Z-shape. Observed in `multi_subgraph_direction_override.mmd` where near-centered edges between ranks exhibit small jogs from even port distribution. |

### 5.3 Crossing and bend optimization requirements

| ID      | Requirement                                                             | Priority | Rationale                                                                   |
| ------- | ----------------------------------------------------------------------- | -------- | --------------------------------------------------------------------------- |
| R-OPT-1 | Crossing avoidance takes priority over bend minimization                | P0       | Purchase (1997): crossings are the single most impactful readability factor |
| R-OPT-2 | Bend minimization takes priority over edge length optimization          | P0       | Purchase (1997): bends rank second                                          |
| R-OPT-3 | Flow-direction port attachment takes priority over bend minimization    | P0       | Flow encoding is the purpose of directional layout (§4.4)                   |
| R-OPT-4 | When crossing is unavoidable, prefer right-angle crossings              | P2       | RAC drawings perform nearly as well as planar drawings                      |
| R-OPT-5 | Post-routing compaction must preserve axis-alignment in orthogonal mode | P0       | Compaction is a quality step, not a correctness-breaking step               |

### 5.4 Backward edge requirements

#### Research basis

Recent work on backward edge routing in hierarchical layouts has converged on a clear consensus across three independent sources:

**VEIL (Schaad, Hoefler & Hoefler, Nov 2025)** formalizes this as **criterion C11: Edge Direction Grouping** — back edges should be grouped on one consistent side of the graph, while forward (skip) edges group on the opposite side. VEIL routes back edges to the **left** side, arguing that in Western reading order the left margin is where the eye returns to start a new line, metaphorically aligning with "going back to the start of a loop." Their key insight: longer backward edges from enforcing happens-before ordering *enhance* comprehension because edge length becomes a semantic signal — the channel height visually encodes how many ranks are spanned. They evaluate this criterion quantitatively across 30 real-world CFGs from Polybench/C, showing improved readability versus both Dagre and Graphviz dot.

**iongraph (Visness/SpiderMonkey, Oct 2025)** independently arrived at the same principle but chose the **opposite side convention**: downward dummies on the left, upward (backward) dummies on the **right**, producing a consistent "counter-clockwise flow." Their stated rationale: this makes it easy to read long vertical edges whose direction would otherwise be ambiguous. They explicitly identify Graphviz's instability on back edge side selection (flipping between passes) as a readability problem.

**Classic Sugiyama/Graphviz** does not pick a side. Back edges are reversed during cycle-breaking, threaded through the normal dummy node and crossing minimization pipeline, then un-reversed at render time. This causes back edges to route *through* the graph rather than *around* it. Both VEIL and iongraph reject this approach for exactly the readability problem mmdflux also observed.

The convergent finding: **which side you pick matters far less than picking one consistently.** All backward edges should use the same channel side within a given layout direction, creating a visual language the reader learns once.

#### Heuristics

**Heuristic 1: Route around, not between.** A backward edge must take a visibly different geometric path than the forward edge it's paired with. For vertically-aligned nodes in TD, the forward edge goes straight down between them; the backward edge must route to one side and come back, creating a loop shape that wraps around the inter-layer space. The test: if you removed arrowheads from both the forward and backward edges, could you still tell which one is the backward edge from path shape alone? If not, the backward routing has failed.

**Heuristic 2: Reverse the flow faces.** Backward edges use the same flow-axis faces as forward edges, but reversed. In TD, forward exits south/enters north; backward exits north/enters south. The arrow ends up pointing against the flow direction, which is the visual signal of counter-flow.

**Heuristic 3: Consistent side channel (C11).** All backward edges in a diagram route through the same side channel. For TD/BT layouts, mmdflux uses the right channel; for LR/RL, the bottom channel. This is the iongraph convention (counter-clockwise flow). The VEIL convention (left channel for TD) is equally valid — consistency is the critical property, not which side. The chosen convention also applies to text rendering, where backward edges route up the right gutter.

**Heuristic 4: Face selection and routing strategy by rank span.** Backward edges use one of two routing strategies depending on how many ranks they span and whether intermediate nodes cause collisions. The threshold defaults to 3 ranks.

*Short backward edges (1–2 ranks):* Use **reversed flow-face attachment with an inline Z-jog**. The edge exits the upstream face, jogs laterally toward the channel side, and enters the downstream face of the target. This produces 3 segments and 2 bends: departure stem → horizontal jog → arrival stem. The horizontal jog direction is always toward the channel side (right in TD/BT, bottom in LR/RL) per Heuristic 3, keeping the backward edge visually separated from the forward edge. No side-channel routing is needed because there are no intermediate nodes to bypass. The 2-bend inline path satisfies Heuristic 1 (distinguishable from forward edge by path shape) while minimizing bends per Purchase (1997).

*Long backward edges (3+ ranks):* Use **side-face attachment with a channeled U-shape**. The edge exits and enters on the channel-side face, with a straight channel run between. This produces 3 segments and 2 bends: departure stem → channel run → arrival stem. The channel routes around all intermediate node bodies at the offset computed by R-BACK-8.

Face selection table by layout direction:

```
Direction  │ Channel side │ Short backward (1–2 ranks)           │ Long backward (3+ ranks)
           │              │ Exit face → Enter face               │ Exit face → Enter face
───────────┼──────────────┼─────────────────────────────────────┼──────────────────────────
TD         │ Right        │ North → South (inline jog right)    │ East → East
BT         │ Right        │ South → North (inline jog right)    │ East → East
LR         │ Bottom       │ West → East (inline jog down)       │ South → South
RL         │ Bottom       │ East → West (inline jog down)       │ South → South
```

Both short and long backward edges produce 3 segments and 2 bends. The difference is in routing strategy: short edges stay inline between source and target with a lateral jog; long edges detour through the side channel with departure/arrival stems perpendicular to the channel run.

*Collision promotion:* If a short backward edge (1–2 ranks) would collide with a node body when routed inline, it must be promoted to side-face (channeled) routing. This applies when a wide intermediate node at an adjacent rank extends into the inline jog space. The collision check uses R-BACK-8's global offset computation. This ensures the threshold is geometry-aware rather than purely count-based.

| ID        | Requirement                                                                                                                                                                                                                                                                                                                                                                                                       | Priority | Rationale                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R-BACK-1  | Backward edges must be visually distinguishable from forward edges through path shape                                                                                                                                                                                                                                                                                                                             | P0       | Cycle identification is a core comprehension task                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| R-BACK-2  | Backward edges use a canonical channel policy: right lane for TD/BT, bottom lane for LR/RL                                                                                                                                                                                                                                                                                                                        | P1       | Deterministic and readable; validated by VEIL C11 criterion and iongraph convention (research 0048 backward-channel findings)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| R-BACK-3  | Backward edge routing must not cross node bodies                                                                                                                                                                                                                                                                                                                                                                  | P0       | Collision avoidance                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| R-BACK-5  | Collision detection for backward edge routing must use shape-aware boundaries, not rectangular bounding boxes, for non-rectangular node shapes (diamond, circle, stadium, etc.)                                                                                                                                                                                                                                   | P1       | A rectangular bounding box underestimates the collision area of a diamond by ~50%. Backward edges routed around a diamond's bbox can cut through the diamond's actual geometry. **Note (c62bbc9):** The `decision.mmd` diamond graze originally attributed to this gap was actually caused by an SVG render-layer issue — `endpoint_attachment_is_invalid()` in `adjust_edge_points_for_shapes()` was hard-coded to reject backward target entries unless near the right face, triggering unwanted reclipping via `intersect_svg_node()` that pulled the terminal lane inward. The router geometry was already correct. Fix: backward target validity now accepts any near-boundary attachment and only reclips when the endpoint drifts into the node interior. The shape-aware collision requirement remains valid as a general principle but has no known triggering fixture as of this update.                                                                                                            |
| R-BACK-4  | Synthetic backward waypoints in float space must produce equivalent grid-snapped output for text rendering                                                                                                                                                                                                                                                                                                        | P1       | Unification correctness gate (research 0047)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| R-BACK-6  | All backward edges within a diagram must route through the same side channel                                                                                                                                                                                                                                                                                                                                      | P0       | VEIL C11 and iongraph both demonstrate that consistent side selection is the critical property for backward edge comprehension. Inconsistent side selection (Graphviz behavior) forces the reader to re-learn the visual language per-edge.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| R-BACK-7  | Backward edge face selection must follow the rank-span decision table in Heuristic 4: short edges (1–2 ranks) use reversed flow-face attachment with an inline 2-bend Z-jog toward the channel side; long edges (≥3 ranks) use same-face attachment on the channel-side face with a 2-bend channeled U-shape. A short edge is promoted to channeled (side-face) if inline routing would collide with a node body. | P1       | See Heuristic 4 face selection table for the specific exit/enter faces per layout direction (TD/BT/LR/RL). Both strategies produce 3 segments and 2 bends — the minimum for a non-straight backward edge. Short edges stay inline with a lateral jog (V-H-V shape); long edges detour through the side channel. The inline Z-jog satisfies Heuristic 1 (path distinguishability) while minimizing bends per Purchase (1997). Side-face channeling for long edges keeps them spatially separated from forward flow and uses channel height as a semantic signal for span distance (VEIL finding). Observed in `git_workflow.mmd` TD rendering: a 4-rank backward edge incorrectly uses North → South (reversed flow-face) instead of East → East (side-face), producing unnecessary bends and a collision with the Staging Area node body. Also observed in `simple_cycle.mmd` TD rendering (A → B → A): a 1-rank backward edge uses 4 bends via side-channel loop instead of the correct 2-bend inline Z-jog. |
| R-BACK-8  | The side-channel offset for backward edges must be computed as a global maximum across all intermediate ranks, not just the source and target nodes                                                                                                                                                                                                                                                               | P0       | The channel must clear every node body between the source and target ranks. In TD, the right channel’s X coordinate is `max(right_edge of all nodes from source rank to target rank) + channel_padding`. In LR, the bottom channel’s Y coordinate is `max(bottom_edge of all nodes from source rank to target rank) + channel_padding`. Computing offset from only the source/target nodes produces collisions with wider intermediate nodes. Observed in `git_workflow.mmd` TD rendering where the right-channel backward edge conflicts with the Staging Area node body.                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| R-BACK-9  | All backward edges must have exactly 2 bends and 3 segments (V-H-V in TD/BT, H-V-H in LR/RL). For short backward edges this is a Z-jog: departure stem → lateral jog toward channel side → arrival stem. For long backward edges this is a U-shape: departure stem → channel run → arrival stem.                                                                                                                  | P0       | 2 bends is the minimum for a non-straight backward edge that must be visually offset from the forward edge (Heuristic 1). Any additional bends are waste that degrades readability per Purchase (1997) and Ware et al (2002). Observed in `git_workflow.mmd` LR rendering where the backward edge has extra bends due to an asymmetric downward dip near the target, in the TD rendering where an outward-then-inward jog adds unnecessary bends, and in `simple_cycle.mmd` where a 1-rank backward edge uses 4 bends instead of 2.                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| R-BACK-10 | The channel run segment of a backward edge must maintain uniform offset from the node envelope along its entire length                                                                                                                                                                                                                                                                                            | P1       | VEIL’s layout produces uniform-offset channels where the distance from the channel to the nearest node body is consistent. Asymmetric clearance (hugging close to the source but dipping away near the target, or vice versa) introduces unnecessary bends and breaks the visual regularity that makes the channel readable as a single continuous path. The channel offset is set by R-BACK-8; the channel run must hold that offset as a constant for the full span between source and target departure/arrival bends.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

### 5.5 Label placement requirements

| ID        | Requirement                                                           | Priority | Rationale                                                                               |
| --------- | --------------------------------------------------------------------- | -------- | --------------------------------------------------------------------------------------- |
| R-LABEL-1 | Edge labels are anchored to the midpoint of the longest segment       | P1       | Maximizes label readability                                                             |
| R-LABEL-2 | Label anchors must be revalidated after path normalization/compaction | P0       | Post-normalization can remove the segment context (research 0048 label-anchor findings) |
| R-LABEL-3 | Labels must not overlap node bodies or other labels                   | P1       | Basic collision avoidance                                                               |
| R-LABEL-4 | Multi-edge label spacing must survive path compaction                 | P1       | Prevents label overlap on parallel edges (research 0049)                                |

### 5.6 Unification requirements

| ID      | Requirement                                                                                           | Priority | Rationale                                                |
| ------- | ----------------------------------------------------------------------------------------------------- | -------- | -------------------------------------------------------- |
| R-UNI-1 | Routing strategy must be separated from output format                                                 | P0       | Core architectural principle (research 0047)             |
| R-UNI-2 | Float-first shared routing engine with grid-snapping for text output                                  | P0       | Only viable coordinate bridging strategy (research 0047) |
| R-UNI-3 | Both text and SVG renderers consume the same `RoutedGraphGeometry` IR                                 | P0       | Already in place                                         |
| R-UNI-4 | Orthogonal router must be promotable to default behind a feature flag with rollback to `full-compute` | P0       | Plan 0076 acceptance gate                                |
| R-UNI-5 | Shared attachment planning using float fractions (0.0–1.0)                                            | P1       | Highest-value single extraction (research 0047)          |

---

## 6. Design Decisions

### DD-1: Priority ordering of routing constraints

When routing constraints conflict, resolve in this order:

1. **Collision avoidance** — edges must not cross node bodies
2. **Flow-direction port attachment** — edges exit downstream face, enter upstream face
3. **Crossing minimization** — reduce edge-edge crossings
4. **Bend minimization** — reduce direction changes
5. **Edge length** — shorter is better, all else equal
6. **Symmetry** — balanced layouts preferred

This ordering is derived from the research hierarchy (§4.1) combined with the flow-direction convention (§4.4). Flow direction is elevated above crossing minimization because in a hierarchical layout, violating flow direction destroys the visual semantics that justify choosing a directional layout in the first place.

### DD-2: Orthogonal routing is the canonical routing strategy

All edge styles share the same upstream routing logic through the orthogonal path builder. Style-specific rendering is a downstream visual treatment:

- **Orthogonal:** render axis-aligned segments directly
- **Rounded:** apply corner radii at bends
- **Straight:** orthogonal routing for correctness, then diagonal simplification as a post-processing step that collapses orthogonal waypoint sequences into straight segments where collision-safe (see DD-4); for text, not applicable (inherently orthogonal)
- **Curved:** interpolate smooth curves through orthogonal waypoints

This means the orthogonal path builder is the critical path for quality — all styles inherit its bend count, crossing behavior, and port attachment decisions.

### DD-3: The `curved` default for SVG is a Mermaid-compatibility choice, not a readability-optimal choice

The research clearly shows straight/low-curvature edges outperform curved edges for task performance. The `curved` default exists because:

- Mermaid.js renders curved edges by default
- Users expect visual consistency when migrating from Mermaid.js
- Aesthetic preference for curves is real even if task performance is lower

For users who prioritize readability over visual similarity to Mermaid.js, `orthogonal` or `straight` should be recommended.

### DD-4: Straight style uses orthogonal routing with diagonal simplification as a post-processing step

The straight edge style has a two-phase pipeline:

1. **Route orthogonally** — use the same shared orthogonal path builder as all other styles. This guarantees correct flow-direction port attachment, departure stems, collision avoidance, and crossing behavior.
2. **Simplify to diagonals** — as a downstream rendering step, collapse orthogonal waypoint sequences into straight diagonal segments where doing so does not cross node bodies or violate collision constraints.

This means straight style is *not* "draw straight lines through raw (start, midpoint, end) waypoints" (which produces poor flow-direction encoding) and is *not* "render orthogonal paths without corner rounding" (which produces no visual distinction from orthogonal style).

For a TD diamond fan (Start → Left, Start → Right, Left → End, Right → End), the ideal straight output is four single-segment diagonal lines — 0 bends each, maximum readability per Xu et al. (2012). The orthogonal routing phase ensures correctness; the diagonal simplification phase delivers the straight visual identity.

The simplification step must:
- Preserve departure stems and terminal approach segments when the diagonal would produce too shallow an angle (near-horizontal exit in a TD layout still breaks flow-direction encoding)
- Reject diagonals that would cross node bodies (fall back to the orthogonal path)
- Maintain label anchor validity after simplification (R-LABEL-2 applies here too)

Observed in `diamond_fan.mmd`: full-compute straight draws diagonals through raw waypoints with weak flow encoding; orthogonal-preview straight renders identical to orthogonal with no visual distinction. Neither is correct. The two-phase pipeline resolves both problems.

### DD-5: Text output is always orthogonal

The character grid imposes axis-aligned rendering. Text routing uses discrete grid coordinates with box-drawing characters. The orthogonal router produces float-coordinate paths that are grid-snapped for text output. This is not a limitation to apologize for — orthogonal is the most readable style for dense information diagrams.

---

## 7. Quality Metrics

### 7.1 Per-edge metrics

| Metric                      | Target                                                | Measurement                                                      |
| --------------------------- | ----------------------------------------------------- | ---------------------------------------------------------------- |
| Bend count                  | Minimize; ≤2 for simple forward edges                 | Count direction changes in resolved path                         |
| Crossing count              | Minimize; 0 for simple layouts                        | Count edge-edge intersections                                    |
| Terminal approach length    | ≥ arrowhead size × 1.5                                | Measure last segment length                                      |
| Departure stem length       | ≥ minimum visible threshold before lateral jog        | Measure first segment length in flow direction                   |
| Axis alignment (orthogonal) | 100% of segments                                      | Verify dx=0 or dy=0 for each segment                             |
| Port corner inset           | ≥ minimum visible margin from nearest corner          | Measure distance from port x/y to nearest node corner coordinate |
| Edge overlap count          | 0 coincident segments for orthogonal/rounded/straight | Count segment pairs sharing identical pixel paths                |

### 7.2 Per-diagram metrics

| Metric                     | Target                                | Measurement                                  |
| -------------------------- | ------------------------------------- | -------------------------------------------- |
| Total crossings            | Minimize                              | Sum of edge-edge intersections               |
| Total bends                | Minimize                              | Sum of bend counts across all edges          |
| Bounding box inflation     | ≤10% beyond node envelope             | Compare route envelope to node-only envelope |
| Port attachment compliance | 100% flow-direction for forward edges | Audit exit/entry face per edge               |

### 7.3 Parity metrics (orthogonal vs full-compute)

| Metric                 | Gate                                 | Measurement                 |
| ---------------------- | ------------------------------------ | --------------------------- |
| Structural equivalence | Must match for simple fixtures       | Same waypoint topology      |
| Visual delta           | Classified and accepted              | SVG diff sweep (script 08)  |
| Determinism            | Identical output for identical input | Hash comparison across runs |

---

## 8. Implementation Roadmap

### Current state (as of 2026-02-16)

- Text routing: mature, orthogonal-only, discrete grid coordinates
- SVG routing: four path styles implemented, `full-compute` default
- Orthogonal routing: in hardening (plans 0076, 0077, 0078)
- Shared primitives: face classification, attachment planning partially extracted

### Active work

| Plan | Focus                                             | Status      |
| ---- | ------------------------------------------------- | ----------- |
| 0076 | Orthogonal routing promotion hardening            | In progress |
| 0077 | Orthogonal routing feedback hardening/remediation | In progress |
| 0078 | Backward edge face normalization follow-up        | In progress |

### Future milestones

| Milestone                         | Depends on                | Description                                                                                                                                      |
| --------------------------------- | ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Orthogonal router promotion       | 0076, 0077, 0078 complete | Make `orthogonal-preview` the default `full-compute` mode                                                                                        |
| Bend cost tuning                  | Orthogonal promotion      | Expose bend cost as a tunable parameter (yFiles pattern)                                                                                         |
| Port constraint API               | Orthogonal promotion      | Allow user-specified port sides for block diagram use cases                                                                                      |
| Style-aware segment constraints   | Orthogonal promotion      | Minimum segment lengths for dotted/thick edges (research 0048 styled-segment findings)                                                           |
| Long skip-edge quality monitoring | Orthogonal promotion      | Keep long-skip outcomes visible through route-envelope and label-drift telemetry in orthogonal-vs-full sweeps (research 0048 long-skip findings) |

---

## 9. Open Research Questions

These questions are not yet resolved and may require spikes or additional research:

1. **Optimal bend cost weighting by diagram type.** Should flowcharts, class diagrams, and future diagram types use different bend cost parameters?

2. **Float-to-grid round-trip fidelity for backward edges.** Can float-space synthetic backward waypoints produce identical grid-snapped text output? (Research 0047, deferred pending spike.)

3. **Face capacity thresholds.** What fan-in/fan-out count triggers face overflow? Current formula does not activate on test fixtures. (Research 0048 face-capacity findings.)

4. **Interaction between bend count and crossing angle.** When a bend reduces a crossing angle toward 90°, is the combined effect positive or negative?

5. **Mixed straight/curved edges in the same diagram.** No research found on performance when forward edges are straight and backward edges are curved (or vice versa).

6. **Impact of edge thickness on bend perception.** Thicker edges may make bends more visually prominent, potentially changing the bend cost calculus for styled edges.

7. **Bounding box metric definition.** Current SVG sweep viewBox deltas are uniformly zero; a stronger metric is needed before gating quality changes. (Research 0048 non-viewBox metric findings.)

8. **Curved-style curvature control from orthogonal waypoints.** Orthogonal-preview curved edges appear better controlled (lower curvature) than full-compute curved edges because the orthogonal waypoints constrain the spline. Should the orthogonal router's tighter waypoint structure be treated as a quality improvement for curved style, or does it over-constrain the curves? Observed in `diamond_fan.mmd`: full-compute produces excessively wide lateral curves; orthogonal-preview produces more restrained curves closer to the low-curvature Lombardi profile that Xu et al. found performs comparably to straight edges.

10. **Shape-aware collision boundaries vs padded bounding boxes.** Shape-aware collision using actual geometry (or convex hull) is more correct but requires a collision polygon per shape. Padded bounding boxes are simpler but waste space — rectangles get unnecessary margin while diamonds need the most. The pragmatic approach may be shape-aware for the small set of Mermaid shapes that differ meaningfully from their bounding box (diamond, circle, stadium) with rectangular bbox as the fallback. Needs profiling to determine whether per-shape collision polygons have measurable cost at scale.

11. **Diagonal simplification minimum angle threshold.** What departure angle (relative to flow direction) is too shallow for straight style? A 45° diagonal from Start to Left in a TD layout still reads as "downward"; a near-horizontal 10° diagonal would not. The threshold needs empirical testing or a heuristic (e.g., minimum 30° from the cross-axis).

---

## 10. References

### Academic

- Purchase, H.C. (1997). "Which aesthetic has the greatest effect on human understanding?" *GD '97*.
- Purchase, H.C., Cohen, R.F., James, M.I. (2002). "An experimental study of the basis for graph drawing algorithms." *JVLC*.
- Xu, K., Rooney, C., Passmore, P., Ham, D.H., Nguyen, P.H. (2012). "A user study on curved edges in graph visualization." *IEEE TVCG*.
- Ware, C., Purchase, H., Colpoys, L., McGill, M. (2002). "Cognitive measurements of graph aesthetics." *Information Visualization*.
- Tamassia, R. (1987). "On embedding a graph in the grid with the minimum number of bends." *SIAM J. Computing*.
- Bar, M., Neta, M. (2006). "Humans prefer curved visual objects." *Psychological Science*.
- Sugiyama, K., Tagawa, S., Toda, M. (1981). "Methods for visual understanding of hierarchical system structures." *IEEE SMC*.
- Spönemann, M., Fuhrmann, H., von Hanxleden, R., Mutzel, P. (2013). "Drawing layered graphs with port constraints." *JVLC*.
- Schaad, P., Hoefler, M., Hoefler, T. (2025). "VEIL: Reading Control Flow Graphs Like Code." *arXiv:2511.05066*. — Formalizes C11 (Edge Direction Grouping): back edges grouped on one side, forward edges on the opposite. Evaluates across 30 Polybench CFGs.
- Gansner, E.R., Koutsofios, E., North, S.C., Vo, K.-P. (1993). "A technique for drawing directed graphs." *IEEE TSE*. — The foundational Graphviz dot paper; describes cycle-breaking via edge reversal and the Sugiyama-based layout pipeline.
- Domrös, S., von Hanxleden, R. (2022). "Preserving Order during Crossing Minimization in Sugiyama Layouts." *VISIGRAPP/IVAPP 2022*. — Documents backward edge ordering problems with model-order-preserving crossing minimization.
- Bennett, C., Ryall, J., Spalteholz, L., Gooch, A. (2007). "The aesthetics of graph visualization." *Computational Aesthetics*. — Survey connecting graph drawing heuristics to Gestalt principles and emotional design.

### Tools and implementations

- Eclipse ELK (Eclipse Layout Kernel): https://eclipse.dev/elk/
- Graphviz dot: https://graphviz.org/
- yFiles: https://www.yworks.com/
- draw.io / diagrams.net: https://www.diagrams.net/
- dagre.js: https://github.com/dagrejs/dagre
- iongraph (SpiderMonkey): https://github.com/mozilla-spidermonkey/iongraph — Visness, B. (2025). "Who needs Graphviz when you can build it yourself?" https://spidermonkey.dev/blog/2025/10/28/iongraph-web.html — Custom hierarchical layout for compiler CFGs; independently validates consistent-side backward edge routing (right channel, counter-clockwise convention).

### Internal references

- Research 0047: Orthogonal routing unification
- Research 0048: Orthogonal routing feedback deep dive
- Research 0049: Backward edge face normalization follow-up
- Plan 0076: Orthogonal routing promotion hardening
- Plan 0077: Orthogonal routing feedback hardening/remediation
- Plan 0078: Backward edge face normalization follow-up
- `docs/ORTHOGONAL_ROUTING_PROMOTION.md`: Promotion checklist and decision record
- `docs/ARCHITECTURE.md`: System architecture overview

---

## Appendix A: Edge Path Style Visual Comparison

```
Orthogonal (axis-aligned segments):

    ┌───┐       ┌───┐
    │ A │       │ B │
    └─┬─┘       └───┘
      │           ▲
      │           │
      └───────────┘

Straight (straight diagonal segments):

    ┌───┐       ┌───┐
    │ A │       │ B │
    └───┘       └───┘
       \         ▲
        \       /
         ------

Rounded (orthogonal with corner radii):

    ┌───┐       ┌───┐
    │ A │       │ B │
    └─┬─┘       └───┘
      │           ▲
      ╰───────────╯

Curved (smooth B-spline curve):

    ┌───┐       ┌───┐
    │ A │       │ B │
    └───┘       └───┘
       ╲         ▲
        ╲ ~~~~~~╱
```

## Appendix B: Port Attachment by Direction

```
TD (top-down):             BT (bottom-up):
    exit: South               exit: North
    ┌───┐                     entry: South
    │ A │                         ▲
    └─┬─┘                     ┌──┴──┐
      │                       │  B  │
      ▼                       └─────┘
    ┌───┐                         │
    │ B │                     ┌───┴─┐
    └───┘                     │  A  │
    entry: North              └─────┘
                              exit: North

LR (left-right):           RL (right-left):
    exit: East                exit: West
    ┌───┐    ┌───┐        ┌───┐    ┌───┐
    │ A ├───►│ B │        │ B │◄───┤ A │
    └───┘    └───┘        └───┘    └───┘
    entry: West               entry: East
```

## Appendix C: Decision Priority Ordering (Quick Reference)

When routing constraints conflict:

```
1. Don't cross node bodies           (collision avoidance)
2. Exit downstream, enter upstream   (flow-direction ports)
3. Reduce edge-edge crossings        (crossing minimization)
4. Reduce bends                      (bend minimization)
5. Shorten edges                     (length optimization)
6. Balance layout                    (symmetry)
```

## Appendix D: Edge Path Style Selection Guide

The four edge styles serve different audiences and contexts. The routing strategy (where waypoints go) and corner treatment (how bends render) are independent axes:

```
                 │ Sharp corners    │ Rounded corners  │ Smooth curves
─────────────────┼──────────────────┼──────────────────┼─────────────────
Orthogonal paths │ orthogonal       │ rounded          │ (n/a)
Diagonal paths   │ straight         │ (rare)           │ (n/a)
Spline paths     │ (n/a)            │ (n/a)            │ curved
```

### Style characteristics

**Orthogonal** (axis-aligned, sharp 90° bends). Every direction change is explicit and unambiguous. Edge crossings are always at right angles, which Purchase found perform nearly as well as planar drawings. The grid-aligned structure makes diagrams scannable — the eye can follow horizontal or vertical channels without tracking arbitrary angles. The tradeoff is visual rigidity: orthogonal diagrams read as precise and authoritative, which is a feature in engineering contexts but can feel clinical for general audiences. This is also the only option for text/ASCII output, where the character grid enforces axis alignment.

**Rounded (orthogonal-rounded)** (axis-aligned paths, arc radii at bends). Identical routing to orthogonal — same waypoints, same bend count, same crossing behavior — with corner arcs as a purely visual treatment. The rounded corners improve edge tracing at bends because the eye follows a curve more naturally than a sharp angle (Bar & Neta 2006 curved-preference finding), while retaining the grid-aligned structure that makes orthogonal scannable. This is the draw.io default and the most popular style in general-purpose diagramming tools.

**Straight** (straight diagonal segments, unconstrained angles). Zero-bend straight lines are the most readable edge style per Xu et al. (2012) — maximum continuity, minimum cognitive load. But straight routing degrades in dense graphs: diagonal crossings occur at arbitrary angles (harder to read than 90° orthogonal crossings), and fan-in/fan-out groups produce visual tangles where multiple diagonals converge. Straight works best for sparse graphs with low edge density where the spatial simplicity of straight lines outweighs the loss of grid structure.

**Curved** (smooth B-spline interpolation through waypoints). Users consistently rate curved edges as the most visually pleasing (Bar & Neta 2006), and smooth splines make diagrams feel organic and approachable. Mermaid.js uses curved edges by default, making this the expected style for users migrating from Mermaid. The tradeoff is measurable: Xu et al. (2012) found more reading errors and slower task completion with heavy curvature compared to straight or low-curvature edges. For diagrams where the audience is scanning an overview rather than tracing individual paths, the aesthetic benefit can outweigh the readability cost.

### Selection guidance

| Context                                                    | Recommended style     | Rationale                                                                    |
| ---------------------------------------------------------- | --------------------- | ---------------------------------------------------------------------------- |
| Engineering reference documentation                        | Orthogonal            | Precision and traceability; every direction change explicit                  |
| General-purpose flowcharts                                 | Rounded               | Approachable feel with structured routing; best all-around compromise        |
| Sparse hierarchies and trees (<15 nodes, low edge density) | Straight              | Minimal visual noise; straight lines maximize readability                    |
| Presentations and stakeholder overviews                    | Curved                | Aesthetic appeal for audiences scanning rather than tracing                  |
| Dense graphs with many crossings                           | Orthogonal or rounded | 90° crossings are significantly more readable than arbitrary-angle crossings |
| Text/terminal output                                       | Orthogonal (inherent) | Character grid constraint; not a choice but a given                          |
| Mermaid.js visual compatibility                            | Curved                | Matches Mermaid.js default rendering conventions                             |

---

*This document is versioned in the mmdflux repository at `docs/EDGE_ROUTING_PRD.md` and updated as research and implementation progress.*
