// MMDS → Excalidraw element conversion
//
// Uses routed-level MMDS when available (polyline edge paths).
// Falls back to layout-level (straight center-to-center arrows).

// MMDS coordinates are unitless layout-space floats. A low SCALE keeps
// edge lengths short while text-based sizing ensures nodes fit their labels.
const SCALE = Number(process.env.SCALE) || 3;

// Edge style: "sharp" (straight segments), "curved" (spline), or "elbow" (orthogonal).
// Controls Excalidraw's roundness + elbowed properties.
type EdgeStyle = "sharp" | "curved" | "elbow";
const EDGE_STYLE: EdgeStyle = (() => {
  const v = (process.env.EDGE_STYLE || "curved").toLowerCase();
  if (v === "sharp" || v === "curved" || v === "elbow") return v;
  return "curved";
})();

// Path detail: how many anchor points to pass to Excalidraw.
//   "endpoints"  — start + end only (2 points)
//   "simplified" — start + midpoint + end (3 points)
//   "full"       — all routed waypoints
type PathDetail = "endpoints" | "simplified" | "full";
const PATH_DETAIL: PathDetail = (() => {
  const v = (process.env.PATH_DETAIL || "full").toLowerCase();
  if (v === "endpoints" || v === "simplified" || v === "full") return v;
  return "full";
})();

// Font sizes (px)
const NODE_FONT_SIZE = 20;
const EDGE_FONT_SIZE = 20;

// Text measurement: approximate character width as fraction of font size.
// Virgil (hand-drawn) averages ~0.55–0.65em; 0.6 is a safe middle ground.
const CHAR_WIDTH_FACTOR = 0.6;

// Padding around text within a node shape (px)
const TEXT_PAD_X = 40;
const TEXT_PAD_Y = 24;

// UML compartment layout (class diagram nodes with --- separator lines)
const COMPARTMENT_PAD_X = 15;
const COMPARTMENT_PAD_Y = 8;

// --- MMDS types (subset) ---

interface MmdsNode {
  id: string;
  label: string;
  shape?: string;
  position: { x: number; y: number };
  size: { width: number; height: number };
  parent?: string;
}

interface MmdsEdge {
  id: string;
  source: string;
  target: string;
  label?: string;
  stroke?: string;
  arrow_start?: string;
  arrow_end?: string;
  path?: [number, number][];
}

interface MmdsSubgraph {
  id: string;
  title?: string;
  children: string[];
}

interface MmdsDefaults {
  node?: { shape?: string };
  edge?: { stroke?: string; arrow_start?: string; arrow_end?: string };
}

export interface MmdsDocument {
  geometry_level?: string;
  nodes: MmdsNode[];
  edges: MmdsEdge[];
  subgraphs?: MmdsSubgraph[];
  defaults?: MmdsDefaults;
}

// --- Excalidraw element type ---

type ExcalidrawElement = Record<string, unknown>;

export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export interface ConvertResult {
  elements: ExcalidrawElement[];
  bounds: Bounds;
}

// --- Helpers ---

function hashCode(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++)
    h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
  return Math.abs(h);
}

function baseProps(id: string): Record<string, unknown> {
  return {
    angle: 0,
    strokeColor: "#1e1e1e",
    backgroundColor: "transparent",
    fillStyle: "solid",
    strokeWidth: 2,
    strokeStyle: "solid",
    roughness: 1,
    opacity: 100,
    groupIds: [],
    frameId: null,
    roundness: null,
    seed: hashCode(id),
    version: 1,
    versionNonce: 0,
    isDeleted: false,
    boundElements: null,
    updated: Date.now(),
    link: null,
    locked: false,
  };
}

function textElement(
  id: string,
  cx: number,
  cy: number,
  cw: number,
  ch: number,
  text: string,
  fontSize: number,
  containerId: string,
): ExcalidrawElement {
  // Estimate text dimensions (~0.6em per char width)
  const estW = text.length * fontSize * 0.6;
  const estH = fontSize * 1.25;
  // Center within container bounds; for point positions (cw=0) use as-is
  const x = cw > 0 ? cx + cw / 2 - estW / 2 : cx;
  const y = ch > 0 ? cy + ch / 2 - estH / 2 : cy;
  return {
    type: "text",
    id,
    x,
    y,
    width: estW,
    height: estH,
    ...baseProps(id),
    text,
    originalText: text,
    fontSize,
    fontFamily: 1,
    textAlign: "center",
    verticalAlign: "middle",
    containerId,
    lineHeight: 1.25,
    autoResize: true,
  };
}

// --- Shape mapping ---

function excalidrawShape(
  mmdsShape: string,
): "rectangle" | "diamond" | "ellipse" {
  switch (mmdsShape) {
    case "diamond":
      return "diamond";
    case "circle":
    case "double-circle":
      return "ellipse";
    default:
      return "rectangle";
  }
}

function excalidrawRoundness(mmdsShape: string): { type: 3 } | null {
  switch (mmdsShape) {
    case "round":
    case "stadium":
      return { type: 3 };
    default:
      return null;
  }
}

// --- Arrow mapping ---

function mapArrowhead(mmdsArrow: string | undefined): string | null {
  switch (mmdsArrow) {
    case "normal":
      return "arrow";
    case "open_triangle":
      return "triangle_outline";
    case "diamond":
      return "diamond";
    case "open_diamond":
      return "diamond_outline";
    case "cross":
      return "bar";
    case "circle":
      return "dot";
    default:
      return null;
  }
}

function mapStrokeStyle(stroke: string | undefined): {
  strokeStyle: "solid" | "dotted";
  strokeWidth: number;
} {
  switch (stroke) {
    case "dotted":
      return { strokeStyle: "dotted", strokeWidth: 2 };
    case "thick":
      return { strokeStyle: "solid", strokeWidth: 4 };
    default:
      return { strokeStyle: "solid", strokeWidth: 2 };
  }
}

// --- Endpoint adjustment for padded nodes ---

// MMDS path endpoints sit at (or near) the original node boundary, which is inside
// the padded shape. This snaps them to the padded boundary using the adjacent path
// segment to determine exit/entry direction.
function adjustEndpoint(
  pt: [number, number],
  adjacentPt: [number, number],
  nodeCx: number,
  nodeCy: number,
  paddedW: number,
  paddedH: number,
): [number, number] {
  const dx = adjacentPt[0] - pt[0];
  const dy = adjacentPt[1] - pt[1];

  if (Math.abs(dy) >= Math.abs(dx)) {
    // Vertical movement: snap y to padded top/bottom edge
    return [pt[0], dy > 0 ? nodeCy + paddedH / 2 : nodeCy - paddedH / 2];
  } else {
    // Horizontal movement: snap x to padded left/right edge
    return [dx > 0 ? nodeCx + paddedW / 2 : nodeCx - paddedW / 2, pt[1]];
  }
}

// --- UML compartment support ---

interface Compartment {
  lines: string[];
  align: "center" | "left";
}

interface CompartmentLayout {
  compartments: Compartment[];
  sectionHeights: number[];
  w: number;
  h: number;
}

// Detect --- separators in a node label and split into UML compartments.
// Returns null if the label has no separators (plain node).
function parseCompartments(label: string): Compartment[] | null {
  const SEP = "---";
  const lines = label.split("\n");
  if (!lines.includes(SEP)) return null;

  const compartments: Compartment[] = [];
  let current: string[] = [];

  for (const line of lines) {
    if (line === SEP) {
      compartments.push({
        lines: current.filter((l) => l.length > 0),
        align: compartments.length === 0 ? "center" : "left",
      });
      current = [];
    } else {
      current.push(line);
    }
  }

  // Final section (after last separator)
  compartments.push({
    lines: current.filter((l) => l.length > 0),
    align: "left",
  });

  return compartments.length >= 2 ? compartments : null;
}

function layoutCompartments(compartments: Compartment[]): CompartmentLayout {
  const lineH = NODE_FONT_SIZE * 1.25;
  const sectionHeights: number[] = [];
  let maxLineW = 0;

  for (const c of compartments) {
    for (const line of c.lines) {
      maxLineW = Math.max(
        maxLineW,
        line.length * NODE_FONT_SIZE * CHAR_WIDTH_FACTOR,
      );
    }
    sectionHeights.push(c.lines.length * lineH + COMPARTMENT_PAD_Y * 2);
  }

  return {
    compartments,
    sectionHeights,
    w: maxLineW + COMPARTMENT_PAD_X * 2,
    h: sectionHeights.reduce((a, b) => a + b, 0),
  };
}

// --- Conversion ---

export function convert(mmds: MmdsDocument): ConvertResult {
  const elements: ExcalidrawElement[] = [];
  const nodeDefaults = mmds.defaults?.node ?? {};
  const edgeDefaults = mmds.defaults?.edge ?? {};

  const nodeMap = new Map<string, MmdsNode>();
  for (const n of mmds.nodes) nodeMap.set(n.id, n);

  // Track boundElements per node (text label + arrow refs)
  const nodeBound = new Map<string, { id: string; type: string }[]>();
  for (const n of mmds.nodes) nodeBound.set(n.id, []);

  // Build subgraph → group ID mapping
  const subgroupIds = new Map<string, string>();
  if (mmds.subgraphs) {
    for (const sg of mmds.subgraphs) {
      subgroupIds.set(sg.id, `group_${sg.id}`);
    }
  }

  // Compute group IDs for a node from its parent chain
  function groupIdsFor(node: MmdsNode): string[] {
    const groups: string[] = [];
    let parentId = node.parent;
    while (parentId) {
      const gid = subgroupIds.get(parentId);
      if (gid) groups.push(gid);
      const parentSg = mmds.subgraphs?.find((sg) => sg.id === parentId);
      parentId = parentSg
        ? mmds.subgraphs?.find((sg) => sg.children.includes(parentSg.id))?.id
        : undefined;
    }
    return groups;
  }

  // Phase 1: parse compartment layouts and compute text-based sizes (scale-independent)
  const compartmentLayouts = new Map<string, CompartmentLayout>();
  const textSizes = new Map<string, { w: number; h: number }>();
  for (const n of mmds.nodes) {
    const compartments = parseCompartments(n.label);
    if (compartments) {
      const layout = layoutCompartments(compartments);
      compartmentLayouts.set(n.id, layout);
      textSizes.set(n.id, { w: layout.w, h: layout.h });
    } else {
      const shape = n.shape ?? nodeDefaults.shape ?? "rectangle";
      const textW = n.label.length * NODE_FONT_SIZE * CHAR_WIDTH_FACTOR;
      const textH = NODE_FONT_SIZE * 1.25;
      let w = textW + TEXT_PAD_X;
      let h = textH + TEXT_PAD_Y;
      if (shape === "diamond") {
        const side = Math.max(w, h);
        w = side;
        h = side;
      }
      textSizes.set(n.id, { w, h });
    }
  }

  // Phase 2: auto-compute minimum scale that prevents node overlap.
  // Layout positions assume unitless float-sized boxes; text-based sizes can be much larger.
  // Two nodes overlap when they overlap on BOTH axes — find the minimum scale
  // that separates every pair on at least one axis.
  const NODE_GAP = 20;
  let scale = SCALE;
  for (let i = 0; i < mmds.nodes.length; i++) {
    for (let j = i + 1; j < mmds.nodes.length; j++) {
      const a = mmds.nodes[i];
      const b = mmds.nodes[j];
      const sa = textSizes.get(a.id);
      const sb = textSizes.get(b.id);
      if (!sa || !sb) continue;
      const dx = Math.abs(a.position.x - b.position.x);
      const dy = Math.abs(a.position.y - b.position.y);
      const needX = (sa.w + sb.w) / 2 + NODE_GAP;
      const needY = (sa.h + sb.h) / 2 + NODE_GAP;

      let minScale = 0;
      if (dx > 0 && dy > 0) {
        minScale = Math.min(needX / dx, needY / dy);
      } else if (dx > 0) {
        minScale = needX / dx;
      } else if (dy > 0) {
        minScale = needY / dy;
      }
      scale = Math.max(scale, minScale);
    }
  }

  // Phase 3: compute final pixel sizes per node using effective scale
  const nodeSizes = new Map<string, { w: number; h: number }>();
  for (const n of mmds.nodes) {
    const layout = compartmentLayouts.get(n.id);
    if (layout) {
      nodeSizes.set(n.id, { w: layout.w, h: layout.h });
    } else {
      const shape = n.shape ?? nodeDefaults.shape ?? "rectangle";
      const textW = n.label.length * NODE_FONT_SIZE * CHAR_WIDTH_FACTOR;
      const textH = NODE_FONT_SIZE * 1.25;
      let w = Math.max(textW, n.size.width * scale) + TEXT_PAD_X;
      let h = Math.max(textH, n.size.height * scale) + TEXT_PAD_Y;
      if (shape === "diamond") {
        const side = Math.max(w, h);
        w = side;
        h = side;
      }
      nodeSizes.set(n.id, { w, h });
    }
  }

  // Bounding box tracking
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;

  function trackBounds(x: number, y: number, w: number, h: number) {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x + w);
    maxY = Math.max(maxY, y + h);
  }

  // --- Nodes ---
  for (const n of mmds.nodes) {
    const shape = n.shape ?? nodeDefaults.shape ?? "rectangle";
    const size = nodeSizes.get(n.id);
    if (!size) continue;
    const { w, h } = size;
    const left = n.position.x * scale - w / 2;
    const top = n.position.y * scale - h / 2;
    const groupIds = groupIdsFor(n);

    const layout = compartmentLayouts.get(n.id);

    if (layout) {
      // UML compartmented node: rectangle + separator lines + per-section text
      const classGroupId = `class_${n.id}`;
      const allGroupIds = [classGroupId, ...groupIds];
      const lineH = NODE_FONT_SIZE * 1.25;

      trackBounds(left, top, w, h);

      // Outer rectangle (text is not bound — positioned manually)
      elements.push({
        type: "rectangle",
        id: n.id,
        x: left,
        y: top,
        width: w,
        height: h,
        ...baseProps(n.id),
        roundness: null,
        groupIds: allGroupIds,
      });

      // Render each compartment's text and separator lines
      let yCursor = top;
      for (let i = 0; i < layout.compartments.length; i++) {
        const c = layout.compartments[i];
        const sectionH = layout.sectionHeights[i];

        if (c.lines.length > 0) {
          const text = c.lines.join("\n");
          const textH = c.lines.length * lineH;
          const textW = Math.max(
            ...c.lines.map(
              (l) => l.length * NODE_FONT_SIZE * CHAR_WIDTH_FACTOR,
            ),
          );
          const textId = `${n.id}_s${i}`;

          const textX =
            c.align === "center"
              ? left + w / 2 - textW / 2
              : left + COMPARTMENT_PAD_X;
          const textY = yCursor + COMPARTMENT_PAD_Y;

          elements.push({
            type: "text",
            id: textId,
            x: textX,
            y: textY,
            width: textW,
            height: textH,
            ...baseProps(textId),
            text,
            originalText: text,
            fontSize: NODE_FONT_SIZE,
            fontFamily: 1,
            textAlign: c.align,
            verticalAlign: "top",
            containerId: null,
            lineHeight: 1.25,
            autoResize: true,
            groupIds: allGroupIds,
          });
        }

        yCursor += sectionH;

        // Separator line between compartments
        if (i < layout.compartments.length - 1) {
          const lineId = `${n.id}_sep${i}`;
          elements.push({
            type: "line",
            id: lineId,
            x: left,
            y: yCursor,
            width: w,
            height: 0,
            ...baseProps(lineId),
            points: [
              [0, 0],
              [w, 0],
            ],
            groupIds: allGroupIds,
          });
        }
      }
    } else {
      // Standard node: single rectangle with bound text label
      const textId = `${n.id}_label`;

      trackBounds(left, top, w, h);
      nodeBound.get(n.id)?.push({ id: textId, type: "text" });

      const el: ExcalidrawElement = {
        type: excalidrawShape(shape),
        id: n.id,
        x: left,
        y: top,
        width: w,
        height: h,
        ...baseProps(n.id),
        roundness: excalidrawRoundness(shape),
      };
      if (groupIds.length > 0) el.groupIds = groupIds;
      elements.push(el);

      const txt = textElement(
        textId,
        left,
        top,
        w,
        h,
        n.label,
        NODE_FONT_SIZE,
        n.id,
      );
      if (groupIds.length > 0) txt.groupIds = groupIds;
      elements.push(txt);
    }
  }

  // --- Edges ---
  for (const e of mmds.edges) {
    const src = nodeMap.get(e.source);
    const tgt = nodeMap.get(e.target);
    if (!src || !tgt) continue;

    const stroke = e.stroke ?? edgeDefaults.stroke ?? "solid";
    const arrowStart = e.arrow_start ?? edgeDefaults.arrow_start ?? "none";
    const arrowEnd = e.arrow_end ?? edgeDefaults.arrow_end ?? "normal";
    const { strokeStyle, strokeWidth } = mapStrokeStyle(stroke);
    const path = e.path;

    let x: number;
    let y: number;
    let points: [number, number][];

    if (path && path.length >= 2) {
      // Convert to pixel coordinates
      const pxPath: [number, number][] = path.map(
        (p) => [p[0] * scale, p[1] * scale] as [number, number],
      );
      // Snap endpoints to node boundaries
      const srcSize = nodeSizes.get(e.source);
      if (!srcSize) continue;
      pxPath[0] = adjustEndpoint(
        pxPath[0],
        pxPath[1],
        src.position.x * scale,
        src.position.y * scale,
        srcSize.w,
        srcSize.h,
      );
      const last = pxPath.length - 1;
      const tgtSize = nodeSizes.get(e.target);
      if (!tgtSize) continue;
      pxPath[last] = adjustEndpoint(
        pxPath[last],
        pxPath[last - 1],
        tgt.position.x * scale,
        tgt.position.y * scale,
        tgtSize.w,
        tgtSize.h,
      );
      x = pxPath[0][0];
      y = pxPath[0][1];
      points = pxPath.map(
        (p) => [p[0] - pxPath[0][0], p[1] - pxPath[0][1]] as [number, number],
      );
    } else {
      const srcCx = src.position.x * scale;
      const srcCy = src.position.y * scale;
      const tgtCx = tgt.position.x * scale;
      const tgtCy = tgt.position.y * scale;
      const srcSize = nodeSizes.get(e.source);
      const tgtSize = nodeSizes.get(e.target);
      if (!srcSize || !tgtSize) continue;
      const start: [number, number] = adjustEndpoint(
        [srcCx, srcCy],
        [tgtCx, tgtCy],
        srcCx,
        srcCy,
        srcSize.w,
        srcSize.h,
      );
      const end: [number, number] = adjustEndpoint(
        [tgtCx, tgtCy],
        [srcCx, srcCy],
        tgtCx,
        tgtCy,
        tgtSize.w,
        tgtSize.h,
      );
      x = start[0];
      y = start[1];
      points = [
        [0, 0],
        [end[0] - start[0], end[1] - start[1]],
      ];
    }

    // Reduce waypoints based on PATH_DETAIL.
    if (PATH_DETAIL === "endpoints" && points.length > 2) {
      points = [points[0], points[points.length - 1]];
    } else if (PATH_DETAIL === "simplified" && points.length > 3) {
      const mid = points[Math.floor(points.length / 2)];
      points = [points[0], mid, points[points.length - 1]];
    }

    for (const p of points) {
      trackBounds(x + p[0], y + p[1], 0, 0);
    }

    const arrowId = e.id;
    const arrowEl: ExcalidrawElement = {
      type: "arrow",
      id: arrowId,
      x,
      y,
      width: Math.abs(points[points.length - 1][0]),
      height: Math.abs(points[points.length - 1][1]),
      ...baseProps(arrowId),
      strokeStyle,
      strokeWidth,
      points,
      startBinding: {
        elementId: e.source,
        fixedPoint: [0.5, 0.5],
        focus: 0,
        gap: 1,
      },
      endBinding: {
        elementId: e.target,
        fixedPoint: [0.5, 0.5],
        focus: 0,
        gap: 1,
      },
      startArrowhead: mapArrowhead(arrowStart),
      endArrowhead: mapArrowhead(arrowEnd),
      roundness: EDGE_STYLE === "curved" ? { type: 2 } : null,
      elbowed: EDGE_STYLE === "elbow",
    };

    if (nodeBound.has(e.source))
      nodeBound.get(e.source)?.push({ id: arrowId, type: "arrow" });
    if (nodeBound.has(e.target))
      nodeBound.get(e.target)?.push({ id: arrowId, type: "arrow" });

    if (e.label) {
      const labelId = `${arrowId}_label`;
      arrowEl.boundElements = [{ id: labelId, type: "text" }];
      elements.push(arrowEl);

      const midIdx = Math.floor(points.length / 2);
      const labelX = x + points[midIdx][0];
      const labelY = y + points[midIdx][1];
      elements.push(
        textElement(
          labelId,
          labelX,
          labelY,
          0,
          0,
          e.label,
          EDGE_FONT_SIZE,
          arrowId,
        ),
      );
    } else {
      elements.push(arrowEl);
    }
  }

  // Patch boundElements onto node shapes
  for (const el of elements) {
    const id = el.id as string;
    const bound = nodeBound.get(id);
    if (bound) {
      el.boundElements = bound;
    }
  }

  const bounds: Bounds = { minX, minY, maxX, maxY };
  return { elements, bounds };
}
