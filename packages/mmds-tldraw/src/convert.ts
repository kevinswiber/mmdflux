import {
  collectSubgraphDescendantNodeIds,
  edgeEndpointTargets,
  type MmdsDocument,
  type MmdsPortFace,
  type MmdsSubgraph,
  type NormalizedMmdsNode,
  type NormalizedMmdsSubgraph,
  normalizeMmds,
} from "@mmds/core";
import {
  createBindingId,
  createShapeId,
  createTLStore,
  type IndexKey,
  type TLRecord,
  type TLStoreSnapshot,
  toRichText,
} from "@tldraw/editor";
import { generateKeyBetween } from "fractional-indexing";

export interface ConvertOptions {
  scale?: number;
  /**
   * Position scale multiplier for node spacing. When omitted, an adaptive
   * ratio is computed from label growth (how much each node expands when
   * tldraw enforces minimum label-based widths). The autoPositionScale()
   * overlap safety net runs regardless.
   */
  nodeSpacing?: number;
}

export interface TldrawConvertResult {
  schema: TLStoreSnapshot["schema"];
  snapshot: TLStoreSnapshot;
  records: TLRecord[];
}

export interface TldrawStoreExport {
  schema: TLStoreSnapshot["schema"];
  records: TLRecord[];
}

export interface TldrawFile {
  tldrawFileFormatVersion: number;
  schema: TLStoreSnapshot["schema"];
  records: TLRecord[];
}

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface Point {
  x: number;
  y: number;
}

const FRAME_PAD_X = 24;
const FRAME_PAD_TOP = 28;
const FRAME_PAD_BOTTOM = 16;

function toShapeId(prefix: string, id: string): string {
  const sanitized = id.replace(/[^A-Za-z0-9_-]/g, "_");
  return createShapeId(`${prefix}_${sanitized}`);
}

function toBindingId(prefix: string, id: string): string {
  const sanitized = id.replace(/[^A-Za-z0-9_-]/g, "_");
  return createBindingId(`${prefix}_${sanitized}`);
}

function clamp01(value: number): number {
  if (value <= 0) return 0;
  if (value >= 1) return 1;
  return value;
}

function distance(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function toPoints(
  path: [number, number][] | undefined,
  scale: number,
): Point[] {
  if (!path || path.length < 2) return [];
  return path.map(([x, y]) => ({ x: x * scale, y: y * scale }));
}

// Approximate char width for tldraw "draw" font at size "m" (~16px).
// Ensures single-line labels don't wrap awkwardly when mmdflux sizes differ from tldraw metrics.
const CHAR_WIDTH_EST = 14;
const MIN_LABEL_PAD_X = 36;
const MIN_LABEL_PAD_Y = 28;

function tldrawEnforcedMinWidth(label: string): number {
  return label.length * CHAR_WIDTH_EST + MIN_LABEL_PAD_X;
}

/**
 * Compute adaptive growth ratio based on how much each node will expand
 * when tldraw enforces minimum label-based widths.
 *
 * Returns the maximum growth ratio across all nodes, clamped to >= 1.0.
 */
function computeAdaptiveGrowthRatio(
  nodes: readonly NormalizedMmdsNode[],
  sizeScale: number,
): number {
  let maxRatio = 1.0;
  for (const node of nodes) {
    const label = node.label ?? node.id;
    const tldrawMinWidth = tldrawEnforcedMinWidth(label);
    const scaledMmdsWidth = node.size.width * sizeScale;
    if (scaledMmdsWidth > 0) {
      const ratio = tldrawMinWidth / scaledMmdsWidth;
      if (ratio > maxRatio) maxRatio = ratio;
    }
  }
  return maxRatio;
}

function scaleNodeRect(
  node: NormalizedMmdsNode,
  sizeScale: number,
  positionScale: number,
): Rect {
  let width = Math.max(8, node.size.width * sizeScale);
  let height = Math.max(8, node.size.height * sizeScale);

  // Ensure minimum size for label so text doesn't wrap to single chars per line.
  const minW = tldrawEnforcedMinWidth(node.label);
  const minH = MIN_LABEL_PAD_Y;
  if (width < minW || height < minH) {
    width = Math.max(width, minW);
    height = Math.max(height, minH);
    // Diamonds need square aspect; ellipses often look better square for short labels.
    if (node.shape === "diamond") {
      const side = Math.max(width, height);
      width = side;
      height = side;
    }
  }

  return {
    x: node.position.x * positionScale - width / 2,
    y: node.position.y * positionScale - height / 2,
    w: width,
    h: height,
  };
}

/**
 * Compute the minimum positionScale so that no pair of nodes overlaps after
 * label-based minimum sizing.  Returns a value >= `basePositionScale`.
 *
 * Nodes are centered at `position * positionScale` with fixed pixel sizes
 * (potentially enlarged for labels).  We find the tightest pair and bump the
 * scale until they no longer overlap with at least MIN_GAP between them.
 */
function autoPositionScale(
  nodes: readonly NormalizedMmdsNode[],
  direction: string | undefined,
  sizeScale: number,
  basePositionScale: number,
): number {
  if (nodes.length < 2) return basePositionScale;

  const horizontal = direction === "LR" || direction === "RL";
  const MIN_GAP = 24;

  // Pre-compute fixed (scale-independent) node sizes.
  const sizes = nodes.map((n) => {
    let w = Math.max(n.size.width * sizeScale, tldrawEnforcedMinWidth(n.label));
    let h = Math.max(n.size.height * sizeScale, MIN_LABEL_PAD_Y);
    if (n.shape === "diamond") {
      const side = Math.max(w, h);
      w = side;
      h = side;
    }
    return { w, h };
  });

  let needed = basePositionScale;

  for (let i = 0; i < nodes.length; i++) {
    for (let j = i + 1; j < nodes.length; j++) {
      const a = nodes[i];
      const b = nodes[j];

      if (horizontal) {
        const dx = Math.abs(a.position.x - b.position.x);
        if (dx < 1) continue;
        // Required scale so rects don't overlap on X.
        const halfW = (sizes[i].w + sizes[j].w) / 2 + MIN_GAP;
        const reqX = halfW / dx;
        if (reqX <= needed) continue;
        // Only matters if they actually overlap on Y at that scale.
        const dy = Math.abs(a.position.y - b.position.y);
        const halfH = (sizes[i].h + sizes[j].h) / 2;
        if (dy * reqX >= halfH) continue;
        needed = reqX;
      } else {
        const dy = Math.abs(a.position.y - b.position.y);
        if (dy < 1) continue;
        const halfH = (sizes[i].h + sizes[j].h) / 2 + MIN_GAP;
        const reqY = halfH / dy;
        if (reqY <= needed) continue;
        const dx = Math.abs(a.position.x - b.position.x);
        const halfW = (sizes[i].w + sizes[j].w) / 2;
        if (dx * reqY >= halfW) continue;
        needed = reqY;
      }
    }
  }

  return needed;
}

function unionRects(rects: Rect[]): Rect {
  const minX = Math.min(...rects.map((r) => r.x));
  const minY = Math.min(...rects.map((r) => r.y));
  const maxX = Math.max(...rects.map((r) => r.x + r.w));
  const maxY = Math.max(...rects.map((r) => r.y + r.h));
  return {
    x: minX,
    y: minY,
    w: maxX - minX,
    h: maxY - minY,
  };
}

function expandRectForFrame(rect: Rect): Rect {
  return {
    x: rect.x - FRAME_PAD_X,
    y: rect.y - FRAME_PAD_TOP,
    w: rect.w + FRAME_PAD_X * 2,
    h: rect.h + FRAME_PAD_TOP + FRAME_PAD_BOTTOM,
  };
}

function withMinSizeAroundCenter(
  rect: Rect,
  width: number,
  height: number,
): Rect {
  if (rect.w >= width && rect.h >= height) return rect;
  const cx = rect.x + rect.w / 2;
  const cy = rect.y + rect.h / 2;
  const nextW = Math.max(rect.w, width);
  const nextH = Math.max(rect.h, height);
  return {
    x: cx - nextW / 2,
    y: cy - nextH / 2,
    w: nextW,
    h: nextH,
  };
}

function mapGeoShape(
  mmdsShape: string,
): "rectangle" | "ellipse" | "diamond" | "hexagon" | "trapezoid" {
  switch (mmdsShape) {
    case "diamond":
      return "diamond";
    case "hexagon":
      return "hexagon";
    case "trapezoid":
    case "inv_trapezoid":
    case "parallelogram":
    case "inv_parallelogram":
    case "manual_input":
    case "asymmetric":
      return "trapezoid";
    case "round":
    case "stadium":
    case "circle":
    case "double_circle":
    case "double-circle":
    case "small_circle":
    case "framed_circle":
    case "crossed_circle":
      return "ellipse";
    default:
      return "rectangle";
  }
}

function mapDash(stroke: string): "solid" | "dotted" {
  return stroke === "dotted" ? "dotted" : "solid";
}

function mapSize(stroke: string): "m" | "l" {
  return stroke === "thick" ? "l" : "m";
}

function mapArrowhead(
  arrow: string,
): "arrow" | "triangle" | "diamond" | "dot" | "bar" | "none" {
  switch (arrow) {
    case "normal":
      return "arrow";
    case "open_triangle":
      return "triangle";
    case "diamond":
    case "open_diamond":
      return "diamond";
    case "circle":
      return "dot";
    case "cross":
      return "bar";
    default:
      return "none";
  }
}

function isTextShape(shape: string): boolean {
  return shape === "text_block" || shape === "text";
}

function isOrthogonal(points: Point[]): boolean {
  if (points.length < 3) return false;
  const tolerance = 0.001;
  for (let i = 1; i < points.length; i++) {
    const dx = Math.abs(points[i].x - points[i - 1].x);
    const dy = Math.abs(points[i].y - points[i - 1].y);
    if (dx > tolerance && dy > tolerance) return false;
  }
  return true;
}

function signedBend(start: Point, end: Point, handle: Point): number {
  const mid = { x: (start.x + end.x) / 2, y: (start.y + end.y) / 2 };
  const delta = { x: end.x - start.x, y: end.y - start.y };
  const perp = { x: -delta.y, y: delta.x };
  const a = { x: mid.x - perp.x, y: mid.y - perp.y };
  const b = { x: mid.x + perp.x, y: mid.y + perp.y };

  const abx = b.x - a.x;
  const aby = b.y - a.y;
  const denom = abx * abx + aby * aby;

  let projected = mid;
  if (denom > 0) {
    const t = ((handle.x - a.x) * abx + (handle.y - a.y) * aby) / denom;
    projected = { x: a.x + abx * t, y: a.y + aby * t };
  }

  let bend = distance(projected, mid);

  const cross =
    (projected.x - end.x) * (mid.y - end.y) -
    (projected.y - end.y) * (mid.x - end.x);
  if (cross < 0) bend *= -1;

  return bend;
}

function elbowMidPoint(points: Point[]): number {
  if (points.length < 3) return 0.5;
  const start = points[0];
  const end = points[points.length - 1];
  const dxTotal = end.x - start.x;
  const dyTotal = end.y - start.y;
  const verticalDominant = Math.abs(dyTotal) >= Math.abs(dxTotal);
  const tolerance = 0.001;

  // For top-down / bottom-up elbows, match the routed horizontal lane when possible.
  // This avoids placing the elbow lane on unrelated node borders.
  if (verticalDominant && Math.abs(dyTotal) > tolerance) {
    let widestHorizontal = -1;
    let laneY: number | null = null;
    for (let i = 1; i < points.length; i++) {
      const a = points[i - 1];
      const b = points[i];
      const segDx = Math.abs(b.x - a.x);
      const segDy = Math.abs(b.y - a.y);
      if (segDy <= tolerance && segDx > tolerance && segDx > widestHorizontal) {
        widestHorizontal = segDx;
        laneY = a.y;
      }
    }
    if (laneY !== null) {
      return clamp01((laneY - start.y) / dyTotal);
    }
  }

  const corner = points[1];
  const total = Math.abs(end.x - start.x) + Math.abs(end.y - start.y);
  if (total <= 0) return 0.5;
  const firstLeg = Math.abs(corner.x - start.x) + Math.abs(corner.y - start.y);
  return clamp01(firstLeg / total);
}

function nudgeAwayFromBorder(
  value: number,
  border: number,
  clearance: number,
): number {
  const delta = value - border;
  if (Math.abs(delta) >= clearance) return value;
  return delta >= 0 ? border + clearance : border - clearance;
}

function nudgeElbowMidPointForBorderCollisions(
  pathPoints: Point[],
  baseMidPoint: number,
  start: Point,
  end: Point,
  fromShapeId: string | undefined,
  toShapeId: string | undefined,
  boundsByShapeId: ReadonlyMap<string, Rect>,
): number {
  if (pathPoints.length < 3) return baseMidPoint;

  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const verticalDominant = Math.abs(dy) >= Math.abs(dx);
  if (!verticalDominant || Math.abs(dy) < 0.001) return baseMidPoint;

  let widestHorizontal = -1;
  let laneXMin = Math.min(start.x, end.x);
  let laneXMax = Math.max(start.x, end.x);
  for (let i = 1; i < pathPoints.length; i++) {
    const a = pathPoints[i - 1];
    const b = pathPoints[i];
    const segDx = Math.abs(b.x - a.x);
    const segDy = Math.abs(b.y - a.y);
    if (segDy <= 0.001 && segDx > 0.001 && segDx > widestHorizontal) {
      widestHorizontal = segDx;
      laneXMin = Math.min(a.x, b.x);
      laneXMax = Math.max(a.x, b.x);
    }
  }

  const clearance = 8;
  let laneY = start.y + dy * baseMidPoint;

  for (const [shapeId, rect] of boundsByShapeId) {
    if (shapeId === fromShapeId || shapeId === toShapeId) continue;
    if (rect.x > laneXMax || rect.x + rect.w < laneXMin) continue;
    laneY = nudgeAwayFromBorder(laneY, rect.y, clearance);
    laneY = nudgeAwayFromBorder(laneY, rect.y + rect.h, clearance);
  }

  const minY = Math.min(start.y, end.y) + 6;
  const maxY = Math.max(start.y, end.y) - 6;
  laneY = Math.max(minY, Math.min(maxY, laneY));
  return clamp01((laneY - start.y) / dy);
}

// Keep edge labels away from endpoints to avoid overlapping nodes.
const LABEL_POS_MIN = 0.25;
const LABEL_POS_MAX = 0.75;

function computeLabelPositionRatio(path: Point[], labelPos?: Point): number {
  if (!labelPos || path.length < 2) return 0.5;

  let total = 0;
  const segmentLengths: number[] = [];
  for (let i = 1; i < path.length; i++) {
    const len = distance(path[i - 1], path[i]);
    segmentLengths.push(len);
    total += len;
  }
  if (total === 0) return 0.5;

  let bestDistance = Number.POSITIVE_INFINITY;
  let bestAlong = total / 2;
  let traversed = 0;

  for (let i = 1; i < path.length; i++) {
    const a = path[i - 1];
    const b = path[i];
    const abx = b.x - a.x;
    const aby = b.y - a.y;
    const denom = abx * abx + aby * aby;

    let t = 0;
    if (denom > 0) {
      t = ((labelPos.x - a.x) * abx + (labelPos.y - a.y) * aby) / denom;
      t = clamp01(t);
    }

    const projected = { x: a.x + abx * t, y: a.y + aby * t };
    const d = distance(projected, labelPos);
    if (d < bestDistance) {
      bestDistance = d;
      bestAlong = traversed + segmentLengths[i - 1] * t;
    }

    traversed += segmentLengths[i - 1];
  }

  const raw = clamp01(bestAlong / total);
  return Math.max(LABEL_POS_MIN, Math.min(LABEL_POS_MAX, raw));
}

/** Intersection of ray A->B with rect boundary. Returns the point on the ray (t>=0) that lies on the rect edge, or null. */
function rayRectIntersection(
  a: Point,
  b: Point,
  rect: Rect,
  segmentOnly: boolean,
): Point | null {
  const { x: rx, y: ry, w: rw, h: rh } = rect;
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  let best: Point | null = null;
  let bestT = segmentOnly ? 2 : Number.POSITIVE_INFINITY;

  const test = (px: number, py: number, qx: number, qy: number) => {
    const ex = qx - px;
    const ey = qy - py;
    const pcx = px - a.x;
    const pcy = py - a.y;
    const denom = dx * ey - dy * ex;
    if (Math.abs(denom) < 1e-10) return;
    const t = (pcx * ey - pcy * ex) / denom;
    const u = (pcx * dy - pcy * dx) / denom;
    const inSegment = segmentOnly ? t >= 0 && t <= 1 : t >= 0;
    if (inSegment && u >= 0 && u <= 1 && t < bestT) {
      bestT = t;
      best = { x: a.x + t * dx, y: a.y + t * dy };
    }
  };

  test(rx, ry, rx + rw, ry);
  test(rx + rw, ry, rx + rw, ry + rh);
  test(rx + rw, ry + rh, rx, ry + rh);
  test(rx, ry + rh, rx, ry);
  return best;
}

function segmentRectIntersection(a: Point, b: Point, rect: Rect): Point | null {
  return rayRectIntersection(a, b, rect, true);
}

/** Project a point inside the rect to the nearest edge based on direction to another point. */
function projectToEdge(
  point: Point,
  directionToward: Point,
  _rect: Rect,
): Point {
  const dx = directionToward.x - point.x;
  const dy = directionToward.y - point.y;

  let nx: number;
  let ny: number;
  if (Math.abs(dx) >= Math.abs(dy)) {
    nx = dx >= 0 ? 1 : 0;
    ny = 0.5;
  } else {
    nx = 0.5;
    ny = dy >= 0 ? 1 : 0;
  }
  return { x: nx, y: ny };
}

/** Pick edge from rect center toward a point (for when terminal is outside). */
function edgeTowardPoint(rect: Rect, point: Point): Point {
  const cx = rect.x + rect.w / 2;
  const cy = rect.y + rect.h / 2;
  const dx = point.x - cx;
  const dy = point.y - cy;
  let nx: number;
  let ny: number;
  if (Math.abs(dx) >= Math.abs(dy)) {
    nx = dx >= 0 ? 1 : 0;
    ny = 0.5;
  } else {
    nx = 0.5;
    ny = dy >= 0 ? 1 : 0;
  }
  return { x: nx, y: ny };
}

/** Project normalized point onto diamond boundary. Diamond has vertices at (0.5,0), (1,0.5), (0.5,1), (0,0.5). */
function projectToDiamondBoundary(nx: number, ny: number): Point {
  const inDiamond = Math.abs(nx - 0.5) + Math.abs(ny - 0.5) <= 0.5;
  if (inDiamond) return { x: nx, y: ny };
  const cx = 0.5;
  const cy = 0.5;
  const dx = nx - cx;
  const dy = ny - cy;
  if (dx >= 0 && dy >= 0) return { x: 0.75, y: 0.75 };
  if (dx >= 0 && dy <= 0) return { x: 0.75, y: 0.25 };
  if (dx <= 0 && dy <= 0) return { x: 0.25, y: 0.25 };
  return { x: 0.25, y: 0.75 };
}

/** Project normalized point onto ellipse boundary (approximate: snap to nearest axis-aligned edge midpoint). */
function projectToEllipseBoundary(nx: number, ny: number): Point {
  const cx = 0.5;
  const cy = 0.5;
  const dx = nx - cx;
  const dy = ny - cy;
  if (Math.abs(dx) >= Math.abs(dy)) {
    return { x: dx >= 0 ? 1 : 0, y: 0.5 };
  }
  return { x: 0.5, y: dy >= 0 ? 1 : 0 };
}

function projectToShapeBoundary(
  p: Point,
  geo: "rectangle" | "ellipse" | "diamond" | "hexagon" | "trapezoid",
): Point {
  if (geo === "rectangle" || geo === "hexagon" || geo === "trapezoid") return p;
  if (geo === "diamond") return projectToDiamondBoundary(p.x, p.y);
  return projectToEllipseBoundary(p.x, p.y);
}

/** Map port face + fraction to tldraw normalizedAnchor (0-1 rect space). */
export function faceAndFractionToNormalizedAnchor(
  face: MmdsPortFace,
  fraction: number,
  geo: "rectangle" | "ellipse" | "diamond" | "hexagon" | "trapezoid",
): Point {
  const f = Math.max(0, Math.min(1, fraction));
  let anchor: Point;
  switch (face) {
    case "top":
      anchor = { x: f, y: 0 };
      break;
    case "bottom":
      anchor = { x: f, y: 1 };
      break;
    case "left":
      anchor = { x: 0, y: f };
      break;
    case "right":
      anchor = { x: 1, y: f };
      break;
  }
  return projectToShapeBoundary(anchor, geo);
}

/** Anchor on the shape edge where the path enters/exits, so arrows attach at the boundary instead of the center. */
function edgeAnchor(
  terminal: Point,
  pathPoints: Point[],
  isStart: boolean,
  rect: Rect | undefined,
  geo: "rectangle" | "ellipse" | "diamond" | "hexagon" | "trapezoid",
): Point {
  if (!rect || rect.w <= 0 || rect.h <= 0) {
    return { x: 0.5, y: 0.5 };
  }

  const other =
    pathPoints.length >= 2
      ? isStart
        ? pathPoints[1]
        : pathPoints[pathPoints.length - 2]
      : null;

  let result: Point;

  if (other) {
    const segHit = isStart
      ? segmentRectIntersection(terminal, other, rect)
      : segmentRectIntersection(other, terminal, rect);
    const rayHit = !segHit && rayRectIntersection(other, terminal, rect, false);
    const hit = segHit ?? rayHit;
    if (hit) {
      const nx = clamp01((hit.x - rect.x) / rect.w);
      const ny = clamp01((hit.y - rect.y) / rect.h);
      const atCorner = (nx <= 0.01 || nx >= 0.99) && (ny <= 0.01 || ny >= 0.99);
      if (atCorner && other) {
        result = edgeTowardPoint(rect, other);
      } else {
        result = { x: nx, y: ny };
      }
    } else {
      const inside =
        terminal.x >= rect.x &&
        terminal.x <= rect.x + rect.w &&
        terminal.y >= rect.y &&
        terminal.y <= rect.y + rect.h;
      if (inside) {
        result = projectToEdge(terminal, other, rect);
      } else {
        result = edgeTowardPoint(rect, other);
      }
    }
    // Direction-based sanity: when path clearly approaches from one side, ensure we use that face.
    // This fixes cases where enlarged rects cause segment-rect intersection to pick the wrong edge.
    // Path segment is (terminal, other) for start or (other, terminal) for end; approach = direction from terminal toward other.
    const approachDx = other.x - terminal.x;
    const approachDy = other.y - terminal.y;
    const tol = 1e-6;
    if (Math.abs(approachDx) > Math.abs(approachDy) + tol) {
      const faceRight = approachDx > 0;
      if (faceRight && result.x < 0.5) result = { x: 1, y: result.y };
      else if (!faceRight && result.x > 0.5) result = { x: 0, y: result.y };
    } else if (Math.abs(approachDy) > tol) {
      const faceBottom = approachDy > 0;
      if (faceBottom && result.y < 0.5) result = { x: result.x, y: 1 };
      else if (!faceBottom && result.y > 0.5) result = { x: result.x, y: 0 };
    }
  } else {
    result = edgeTowardPoint(rect, terminal);
  }

  return projectToShapeBoundary(result, geo);
}

function subgraphDepth(
  subgraph: MmdsSubgraph,
  byId: ReadonlyMap<string, Pick<MmdsSubgraph, "parent">>,
): number {
  let depth = 0;
  let current = subgraph.parent;
  while (current) {
    depth += 1;
    current = byId.get(current)?.parent;
  }
  return depth;
}

function recordSortOrder(typeName: string): number {
  switch (typeName) {
    case "document":
      return 0;
    case "page":
      return 1;
    case "shape":
      return 2;
    case "binding":
      return 3;
    case "asset":
      return 4;
    default:
      return 9;
  }
}

/** Z-order: frames behind nodes, arrows on top. */
function shapeZOrder(type: string): number {
  switch (type) {
    case "frame":
      return 0;
    case "arrow":
      return 2;
    default:
      return 1;
  }
}

function generateSequentialIndices(count: number): IndexKey[] {
  const indices: IndexKey[] = [];
  let prev: string | null = null;
  for (let i = 0; i < count; i++) {
    prev = generateKeyBetween(prev, null);
    indices.push(prev as IndexKey);
  }
  return indices;
}

function sortRecords(records: TLRecord[]): TLRecord[] {
  return [...records].sort((a, b) => {
    const rankDiff = recordSortOrder(a.typeName) - recordSortOrder(b.typeName);
    if (rankDiff !== 0) return rankDiff;
    return String(a.id).localeCompare(String(b.id));
  });
}

function assignShapeIndices(records: TLRecord[]): void {
  const byParent = new Map<string, TLRecord[]>();

  for (const record of records) {
    if (record.typeName !== "shape") continue;
    const parentId = String((record as { parentId: string }).parentId);
    const bucket = byParent.get(parentId) ?? [];
    bucket.push(record);
    byParent.set(parentId, bucket);
  }

  const parentIds = [...byParent.keys()].sort((a, b) => a.localeCompare(b));
  for (const parentId of parentIds) {
    const bucket = byParent.get(parentId);
    if (!bucket) continue;

    bucket.sort((a, b) => {
      const za = shapeZOrder((a as { type: string }).type);
      const zb = shapeZOrder((b as { type: string }).type);
      if (za !== zb) return za - zb;
      return String(a.id).localeCompare(String(b.id));
    });
    const indices = generateSequentialIndices(bucket.length);
    for (let i = 0; i < bucket.length; i++) {
      (bucket[i] as { index: string }).index = indices[i];
    }
  }
}

function resolveEndpointShapeId(
  endpoint: ReturnType<typeof edgeEndpointTargets>["from"],
  frameShapeIdBySubgraphId: ReadonlyMap<string, string>,
  nodeShapeIdByNodeId: ReadonlyMap<string, string>,
): string | undefined {
  if (endpoint.kind === "subgraph") {
    return (
      frameShapeIdBySubgraphId.get(endpoint.id) ??
      nodeShapeIdByNodeId.get(endpoint.node_id)
    );
  }
  return nodeShapeIdByNodeId.get(endpoint.id);
}

function isMmdsDocument(input: unknown): input is MmdsDocument {
  if (!input || typeof input !== "object") return false;
  const maybe = input as Record<string, unknown>;
  return Array.isArray(maybe.nodes) && Array.isArray(maybe.edges);
}

function fallbackFrameRect(
  subgraph: NormalizedMmdsSubgraph,
  positionScale: number,
): Rect {
  const width = Math.max(160, (subgraph.bounds?.width ?? 160) * positionScale);
  const height = Math.max(96, (subgraph.bounds?.height ?? 96) * positionScale);
  return { x: 0, y: 0, w: width, h: height };
}

export function convertToTldraw(
  mmds: MmdsDocument,
  options: ConvertOptions = {},
): TldrawConvertResult {
  const scale = options.scale ?? 1;
  const normalized = normalizeMmds(mmds);
  const adaptiveRatio = computeAdaptiveGrowthRatio(normalized.nodes, scale);
  const nodeSpacing = options.nodeSpacing ?? adaptiveRatio;
  const positionScale = autoPositionScale(
    normalized.nodes,
    normalized.metadata?.direction,
    scale,
    scale * nodeSpacing,
  );

  const store = createTLStore({
    defaultName: normalized.metadata?.diagram_type ?? "MMDS",
  });

  const pageId = "page:page";
  const pageIndex = "a1" as IndexKey;

  const pageRecord: TLRecord = {
    id: pageId,
    typeName: "page",
    name: "Page 1",
    index: pageIndex,
    meta: {},
  } as TLRecord;

  const nodeRectById = new Map<string, Rect>();
  for (const node of normalized.nodes) {
    nodeRectById.set(node.id, scaleNodeRect(node, scale, positionScale));
  }

  const frameShapeIdBySubgraphId = new Map<string, string>();
  for (const subgraph of normalized.subgraphs) {
    frameShapeIdBySubgraphId.set(subgraph.id, toShapeId("sg", subgraph.id));
  }

  const frameRectBySubgraphId = new Map<string, Rect>();

  const computeFrameRect = (subgraphId: string): Rect | undefined => {
    const cached = frameRectBySubgraphId.get(subgraphId);
    if (cached) return cached;

    const subgraph = normalized.subgraph_by_id.get(subgraphId);
    if (!subgraph) return undefined;

    const rects: Rect[] = [];

    const descendantNodeIds = collectSubgraphDescendantNodeIds(
      subgraph.id,
      normalized.nodes,
      normalized.subgraphs,
    );

    for (const nodeId of descendantNodeIds) {
      const rect = nodeRectById.get(nodeId);
      if (rect) rects.push(rect);
    }

    const childSubgraphIds =
      normalized.subgraph_children_by_parent.get(subgraph.id) ?? [];
    for (const childSubgraphId of childSubgraphIds) {
      const childRect = computeFrameRect(childSubgraphId);
      if (childRect) rects.push(childRect);
    }

    let frameRect =
      rects.length > 0
        ? expandRectForFrame(unionRects(rects))
        : fallbackFrameRect(subgraph, positionScale);

    if (subgraph.bounds) {
      frameRect = withMinSizeAroundCenter(
        frameRect,
        Math.max(96, subgraph.bounds.width * positionScale),
        Math.max(64, subgraph.bounds.height * positionScale),
      );
    }

    frameRectBySubgraphId.set(subgraph.id, frameRect);
    return frameRect;
  };

  for (const subgraph of normalized.subgraphs) {
    computeFrameRect(subgraph.id);
  }

  const nodes = [...normalized.nodes].sort((a, b) => a.id.localeCompare(b.id));
  const subgraphs = [...normalized.subgraphs].sort((a, b) => {
    const depthDiff =
      subgraphDepth(a, normalized.subgraph_by_id) -
      subgraphDepth(b, normalized.subgraph_by_id);
    if (depthDiff !== 0) return depthDiff;
    return a.id.localeCompare(b.id);
  });
  const edges = normalized.edges
    .filter((edge) => edge.stroke !== "invisible")
    .sort((a, b) => a.id.localeCompare(b.id));

  const shapeRecords: TLRecord[] = [];
  const bindingRecords: TLRecord[] = [];

  const absoluteBoundsByShapeId = new Map<string, Rect>();
  const geoByShapeId = new Map<
    string,
    "rectangle" | "ellipse" | "diamond" | "hexagon" | "trapezoid"
  >();
  const nodeShapeIdByNodeId = new Map<string, string>();

  for (const node of nodes) {
    nodeShapeIdByNodeId.set(node.id, toShapeId("node", node.id));
  }

  for (const subgraph of subgraphs) {
    const shapeId = frameShapeIdBySubgraphId.get(subgraph.id);
    const absRect = frameRectBySubgraphId.get(subgraph.id);
    if (!shapeId || !absRect) continue;

    const parentAbs = subgraph.parent
      ? frameRectBySubgraphId.get(subgraph.parent)
      : undefined;
    const parentId = subgraph.parent
      ? (frameShapeIdBySubgraphId.get(subgraph.parent) ?? pageId)
      : pageId;

    shapeRecords.push({
      id: shapeId,
      typeName: "shape",
      type: "frame",
      parentId,
      index: "",
      x: parentAbs ? absRect.x - parentAbs.x : absRect.x,
      y: parentAbs ? absRect.y - parentAbs.y : absRect.y,
      rotation: 0,
      isLocked: false,
      opacity: 1,
      props: {
        w: absRect.w,
        h: absRect.h,
        name: subgraph.title ?? subgraph.id,
        color: "grey",
      },
      meta: {},
    } as TLRecord);

    absoluteBoundsByShapeId.set(shapeId, absRect);
    geoByShapeId.set(shapeId, "rectangle");
  }

  for (const node of nodes) {
    const absRect = nodeRectById.get(node.id);
    const shapeId = nodeShapeIdByNodeId.get(node.id);
    if (!absRect || !shapeId) continue;

    const parentAbs = node.parent
      ? frameRectBySubgraphId.get(node.parent)
      : undefined;
    const parentId = node.parent
      ? (frameShapeIdBySubgraphId.get(node.parent) ?? pageId)
      : pageId;

    const common = {
      id: shapeId,
      typeName: "shape",
      parentId,
      index: "",
      x: parentAbs ? absRect.x - parentAbs.x : absRect.x,
      y: parentAbs ? absRect.y - parentAbs.y : absRect.y,
      rotation: 0,
      isLocked: false,
      opacity: 1,
      meta: {},
    };

    if (isTextShape(node.shape)) {
      shapeRecords.push({
        ...common,
        type: "text",
        props: {
          color: "black",
          size: "m",
          font: "draw",
          textAlign: "middle",
          w: Math.max(8, absRect.w),
          richText: toRichText(node.label),
          scale: 1,
          autoSize: false,
        },
      } as TLRecord);
    } else {
      const geo = mapGeoShape(node.shape);
      geoByShapeId.set(shapeId, geo);
      shapeRecords.push({
        ...common,
        type: "geo",
        props: {
          geo,
          dash: "solid",
          url: "",
          w: absRect.w,
          h: absRect.h,
          growY: 0,
          scale: 1,
          labelColor: "black",
          color: "black",
          fill: "none",
          size: "m",
          font: "draw",
          align: "middle",
          verticalAlign: "middle",
          richText: toRichText(node.label),
        },
      } as TLRecord);
    }

    absoluteBoundsByShapeId.set(shapeId, absRect);
  }

  for (const edge of edges) {
    const edgeShapeId = toShapeId("edge", edge.id);
    const endpointTargets = edgeEndpointTargets(edge);

    const src = normalized.node_by_id.get(edge.source);
    const tgt = normalized.node_by_id.get(edge.target);
    if (!src || !tgt) continue;

    const routedPoints = toPoints(edge.path, positionScale);
    const hasRoutedPath = routedPoints.length >= 2;
    const start =
      routedPoints.length >= 2
        ? routedPoints[0]
        : {
            x: src.position.x * positionScale,
            y: src.position.y * positionScale,
          };
    const end =
      routedPoints.length >= 2
        ? routedPoints[routedPoints.length - 1]
        : {
            x: tgt.position.x * positionScale,
            y: tgt.position.y * positionScale,
          };

    const pathPoints = routedPoints.length >= 2 ? routedPoints : [start, end];
    const localPath = pathPoints.map((point) => ({
      x: point.x - start.x,
      y: point.y - start.y,
    }));

    const fromShapeId = resolveEndpointShapeId(
      endpointTargets.from,
      frameShapeIdBySubgraphId,
      nodeShapeIdByNodeId,
    );
    const toShapeIdResolved = resolveEndpointShapeId(
      endpointTargets.to,
      frameShapeIdBySubgraphId,
      nodeShapeIdByNodeId,
    );

    const kind = isOrthogonal(pathPoints) ? "elbow" : "arc";
    const elbowMidPointValue =
      kind === "elbow"
        ? nudgeElbowMidPointForBorderCollisions(
            pathPoints,
            elbowMidPoint(pathPoints),
            start,
            end,
            fromShapeId,
            toShapeIdResolved,
            absoluteBoundsByShapeId,
          )
        : 0.5;

    const handle =
      localPath.length > 2
        ? localPath[Math.floor(localPath.length / 2)]
        : {
            x: (end.x - start.x) / 2,
            y: (end.y - start.y) / 2,
          };

    const labelPos = edge.label_position
      ? {
          x: edge.label_position.x * positionScale,
          y: edge.label_position.y * positionScale,
        }
      : undefined;

    shapeRecords.push({
      id: edgeShapeId,
      typeName: "shape",
      type: "arrow",
      parentId: pageId,
      index: "",
      x: start.x,
      y: start.y,
      rotation: 0,
      isLocked: false,
      opacity: 1,
      props: {
        kind,
        labelColor: "black",
        color: "black",
        fill: "none",
        dash: mapDash(edge.stroke),
        size: mapSize(edge.stroke),
        arrowheadStart: mapArrowhead(edge.arrow_start),
        arrowheadEnd: mapArrowhead(edge.arrow_end),
        font: "draw",
        start: { x: 0, y: 0 },
        end: { x: end.x - start.x, y: end.y - start.y },
        bend:
          kind === "arc"
            ? signedBend(
                { x: 0, y: 0 },
                { x: end.x - start.x, y: end.y - start.y },
                handle,
              )
            : 0,
        richText: toRichText(edge.label ?? ""),
        labelPosition: computeLabelPositionRatio(pathPoints, labelPos),
        scale: edge.label ? 1.5 : 1,
        elbowMidPoint: elbowMidPointValue,
      },
      meta: {},
    } as TLRecord);

    if (fromShapeId) {
      const fromRect = absoluteBoundsByShapeId.get(fromShapeId);
      const fromGeo = geoByShapeId.get(fromShapeId) ?? "rectangle";
      const startPort = edge.source_port;
      const startAnchor = hasRoutedPath
        ? edgeAnchor(start, pathPoints, true, fromRect, fromGeo)
        : startPort
          ? faceAndFractionToNormalizedAnchor(
              startPort.face,
              startPort.fraction,
              fromGeo,
            )
          : edgeAnchor(start, pathPoints, true, fromRect, fromGeo);
      bindingRecords.push({
        id: toBindingId("edge_start", edge.id),
        typeName: "binding",
        type: "arrow",
        fromId: edgeShapeId,
        toId: fromShapeId,
        props: {
          terminal: "start",
          normalizedAnchor: startAnchor,
          isExact: false,
          isPrecise: true,
          snap: kind === "elbow" ? "edge" : "none",
        },
        meta: {},
      } as TLRecord);
    }

    if (toShapeIdResolved) {
      const toRect = absoluteBoundsByShapeId.get(toShapeIdResolved);
      const toGeo = geoByShapeId.get(toShapeIdResolved) ?? "rectangle";
      const endPort = edge.target_port;
      const endAnchor = hasRoutedPath
        ? edgeAnchor(end, pathPoints, false, toRect, toGeo)
        : endPort
          ? faceAndFractionToNormalizedAnchor(
              endPort.face,
              endPort.fraction,
              toGeo,
            )
          : edgeAnchor(end, pathPoints, false, toRect, toGeo);
      bindingRecords.push({
        id: toBindingId("edge_end", edge.id),
        typeName: "binding",
        type: "arrow",
        fromId: edgeShapeId,
        toId: toShapeIdResolved,
        props: {
          terminal: "end",
          normalizedAnchor: endAnchor,
          isExact: false,
          isPrecise: true,
          snap: kind === "elbow" ? "edge" : "none",
        },
        meta: {},
      } as TLRecord);
    }
  }

  assignShapeIndices(shapeRecords);

  const allRecords = [pageRecord, ...shapeRecords, ...bindingRecords];
  store.put(allRecords);

  const snapshot = store.getStoreSnapshot();
  const records = sortRecords(Object.values(snapshot.store) as TLRecord[]);
  store.dispose();

  return {
    schema: snapshot.schema,
    snapshot,
    records,
  };
}

export function convertToTldrawStore(
  mmds: MmdsDocument,
  options: ConvertOptions = {},
): TldrawStoreExport {
  const { schema, records } = convertToTldraw(mmds, options);
  return { schema, records };
}

export function toTldrawFile(
  input: MmdsDocument | TldrawStoreExport,
  options: ConvertOptions = {},
): TldrawFile {
  const store = isMmdsDocument(input)
    ? convertToTldrawStore(input, options)
    : input;
  return {
    tldrawFileFormatVersion: 1,
    schema: store.schema,
    records: store.records,
  };
}
