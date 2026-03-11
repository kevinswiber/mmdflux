import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { normalizeMmds } from "@mmds/core";
import { createTLStore, parseTldrawJsonFile } from "tldraw";
import {
  convertToTldraw,
  convertToTldrawStore,
  faceAndFractionToNormalizedAnchor,
  toTldrawFile,
} from "../dist/convert.js";

const repoRoot = path.resolve(process.cwd(), "../..");

function fixture(...segments) {
  const fullPath = path.join(repoRoot, ...segments);
  return JSON.parse(fs.readFileSync(fullPath, "utf8"));
}

function importPackage(specifier, expression) {
  return spawnSync(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `const mod = await import(${JSON.stringify(specifier)}); console.log(JSON.stringify(${expression})); process.exit(0);`,
    ],
    {
      cwd: process.cwd(),
      encoding: "utf8",
    },
  );
}

function assertParses(file) {
  const schema = createTLStore().schema;
  const parsed = parseTldrawJsonFile({
    json: JSON.stringify(file),
    schema,
  });

  assert.equal(parsed.ok, true);
}

test("tldraw library entrypoint is side-effect free", () => {
  const result = importPackage(
    "@mmds/tldraw",
    `{
    convertToTldrawStore: typeof mod.convertToTldrawStore,
    hasMain: "main" in mod,
  }`,
  );

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.deepEqual(JSON.parse(result.stdout), {
    convertToTldrawStore: "function",
    hasMain: false,
  });
});

test("tldraw CLI entrypoint owns the runtime main", () => {
  const result = importPackage(
    "@mmds/tldraw/cli",
    `{
    main: typeof mod.main,
    hasConvertToTldrawStore: "convertToTldrawStore" in mod,
  }`,
  );

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.deepEqual(JSON.parse(result.stdout), {
    main: "function",
    hasConvertToTldrawStore: false,
  });
});

test("tldraw CLI runs when invoked through a symlinked bin path", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "mmds-tldraw-cli-"));
  const linkPath = path.join(tmpDir, "mmds-to-tldraw.mjs");
  fs.symlinkSync(path.resolve(process.cwd(), "dist/cli.js"), linkPath);

  try {
    const result = spawnSync(process.execPath, [linkPath], {
      cwd: process.cwd(),
      encoding: "utf8",
      input: JSON.stringify(
        fixture("tests", "fixtures", "mmds", "positioned", "routed-basic.json"),
      ),
      timeout: 5_000,
    });

    assert.equal(
      result.status,
      0,
      result.stderr || result.stdout || result.error?.message,
    );

    const parsed = JSON.parse(result.stdout);
    assert.equal(parsed.tldrawFileFormatVersion, 1);
    assert.ok(Array.isArray(parsed.records));
    assert.ok(parsed.records.length > 0);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("produces a .tldr envelope that parses with current tldraw parser", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "positioned",
    "layout-basic.json",
  );
  const file = toTldrawFile(mmds);

  assert.equal(file.tldrawFileFormatVersion, 1);
  assert.ok(Array.isArray(file.records));
  assertParses(file);
});

test("shared flowchart contract fixture converts to a parseable .tldr file", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "contracts",
    "flowchart-simple.layout.json",
  );
  const file = toTldrawFile(mmds);

  assertParses(file);
  const edge = file.records.find(
    (record) =>
      record.typeName === "shape" &&
      record.type === "arrow" &&
      record.id === "shape:edge_e0",
  );
  assert.ok(edge);
});

test("fixture integration: layout and routed basics parse and emit arrows", () => {
  const layout = fixture(
    "tests",
    "fixtures",
    "mmds",
    "positioned",
    "layout-basic.json",
  );
  const routed = fixture(
    "tests",
    "fixtures",
    "mmds",
    "positioned",
    "routed-basic.json",
  );

  const layoutFile = toTldrawFile(layout);
  const routedFile = toTldrawFile(routed);

  assertParses(layoutFile);
  assertParses(routedFile);

  const routedArrow = routedFile.records.find(
    (record) =>
      record.typeName === "shape" &&
      record.type === "arrow" &&
      record.id === "shape:edge_e0",
  );
  assert.ok(routedArrow);
  assert.equal(routedArrow.props.kind, "arc");
});

test("shared MMDS profile fixtures remain consumable by the tldraw adapter", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "profiles",
    "profiles-svg-v1.json",
  );

  const file = toTldrawFile(mmds);

  assertParses(file);
  assert.ok(file.records.length > 0);
});

test("omits invisible edges from emitted tldraw shape records", () => {
  const mmds = {
    version: 1,
    geometry_level: "layout",
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    metadata: {
      diagram_type: "flowchart",
      direction: "TD",
      bounds: { width: 200, height: 120 },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 20, y: 20 },
        size: { width: 40, height: 20 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 120, y: 20 },
        size: { width: 40, height: 20 },
      },
    ],
    edges: [
      {
        id: "e0",
        source: "A",
        target: "B",
        stroke: "invisible",
        arrow_start: "none",
        arrow_end: "none",
      },
      {
        id: "e1",
        source: "A",
        target: "B",
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
      },
    ],
  };

  const converted = convertToTldraw(mmds);
  const arrowShapes = converted.records.filter(
    (record) => record.typeName === "shape" && record.type === "arrow",
  );

  assert.equal(arrowShapes.length, 1);
  assert.equal(arrowShapes[0].id, "shape:edge_e1");
});

test("maps stroke and arrowhead styles for arrow shapes", () => {
  const mmds = {
    version: 1,
    geometry_level: "layout",
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    metadata: {
      diagram_type: "flowchart",
      direction: "LR",
      bounds: { width: 240, height: 120 },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 20, y: 20 },
        size: { width: 40, height: 20 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 120, y: 20 },
        size: { width: 40, height: 20 },
      },
      {
        id: "C",
        label: "C",
        position: { x: 200, y: 20 },
        size: { width: 40, height: 20 },
      },
    ],
    edges: [
      {
        id: "e0",
        source: "A",
        target: "B",
        stroke: "dotted",
        arrow_start: "circle",
        arrow_end: "open_triangle",
      },
      {
        id: "e1",
        source: "B",
        target: "C",
        stroke: "thick",
        arrow_start: "cross",
        arrow_end: "diamond",
      },
    ],
  };

  const converted = convertToTldraw(mmds);
  const e0 = converted.records.find(
    (record) =>
      record.typeName === "shape" &&
      record.type === "arrow" &&
      record.id === "shape:edge_e0",
  );
  const e1 = converted.records.find(
    (record) =>
      record.typeName === "shape" &&
      record.type === "arrow" &&
      record.id === "shape:edge_e1",
  );

  assert.ok(e0);
  assert.equal(e0.props.dash, "dotted");
  assert.equal(e0.props.arrowheadStart, "dot");
  assert.equal(e0.props.arrowheadEnd, "triangle");

  assert.ok(e1);
  assert.equal(e1.props.size, "l");
  assert.equal(e1.props.arrowheadStart, "bar");
  assert.equal(e1.props.arrowheadEnd, "diamond");
});

test("maps subgraphs to frame shapes and uses frame bindings for endpoint intent", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "subgraph-endpoint-subgraph-to-subgraph-present.json",
  );

  const converted = convertToTldraw(mmds);
  const frames = converted.records.filter(
    (record) => record.typeName === "shape" && record.type === "frame",
  );
  assert.ok(frames.length >= 2);

  const edgeShapeId = "shape:edge_e2";
  const bindingsForEdge = converted.records.filter(
    (record) => record.typeName === "binding" && record.fromId === edgeShapeId,
  );

  assert.ok(bindingsForEdge.length >= 2);
  for (const binding of bindingsForEdge) {
    assert.ok(String(binding.toId).startsWith("shape:sg_"));
  }
});

test("nests child frames under parent frames from subgraph.parent", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "layout-with-subgraphs.json",
  );
  const converted = convertToTldraw(mmds);

  const parentFrame = converted.records.find(
    (record) =>
      record.typeName === "shape" &&
      record.type === "frame" &&
      record.id === "shape:sg_sg1",
  );
  const childFrame = converted.records.find(
    (record) =>
      record.typeName === "shape" &&
      record.type === "frame" &&
      record.id === "shape:sg_sg2",
  );

  assert.ok(parentFrame);
  assert.ok(childFrame);
  assert.equal(childFrame.parentId, "shape:sg_sg1");
});

test("shape mapping matrix fixture keeps diamond node and dotted edge", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "generation",
    "shapes-and-strokes.json",
  );
  const converted = convertToTldraw(mmds);

  const geoShapes = converted.records.filter(
    (record) => record.typeName === "shape" && record.type === "geo",
  );
  const decisionShape = geoShapes.find(
    (shape) => shape.id === "shape:node_Decision",
  );
  assert.ok(decisionShape);
  assert.equal(decisionShape.props.geo, "diamond");

  const edge = converted.records.find(
    (record) =>
      record.typeName === "shape" &&
      record.type === "arrow" &&
      record.id === "shape:edge_e0",
  );
  assert.ok(edge);
  assert.equal(edge.props.dash, "dotted");
});

test("class layout fixture integration emits parseable .tldr", () => {
  const mmds = fixture("tests", "fixtures", "mmds", "layout-valid-class.json");
  const file = toTldrawFile(mmds);

  assertParses(file);
  const nodeShape = file.records.find(
    (record) => record.typeName === "shape" && record.id === "shape:node_User",
  );
  assert.ok(nodeShape);
});

test("adaptive spacing: short-label LR diagrams are tighter than static 1.5x", () => {
  const mmds = {
    version: 1,
    geometry_level: "layout",
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    metadata: {
      diagram_type: "flowchart",
      direction: "LR",
      bounds: { width: 400, height: 120 },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 50, y: 60 },
        size: { width: 80, height: 40 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 200, y: 60 },
        size: { width: 80, height: 40 },
      },
      {
        id: "C",
        label: "C",
        position: { x: 350, y: 60 },
        size: { width: 80, height: 40 },
      },
    ],
    edges: [],
  };

  // Default (adaptive) vs explicit static 1.5x
  const defaultResult = convertToTldraw(mmds);
  const staticResult = convertToTldraw(mmds, { nodeSpacing: 1.5 });

  const geos = (result) =>
    result.records.filter((r) => r.typeName === "shape" && r.type === "geo");
  const xExtent = (shapes) => {
    const min = Math.min(...shapes.map((s) => s.x));
    const max = Math.max(...shapes.map((s) => s.x + s.props.w));
    return max - min;
  };

  const defaultExtent = xExtent(geos(defaultResult));
  const staticExtent = xExtent(geos(staticResult));

  assert.ok(
    defaultExtent < staticExtent,
    `Expected adaptive extent (${defaultExtent}) < static 1.5x extent (${staticExtent})`,
  );
});

test("adaptive spacing: long-label TD diagrams space more than static 1.2x", () => {
  const mmds = {
    version: 1,
    geometry_level: "layout",
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    metadata: {
      diagram_type: "flowchart",
      direction: "TD",
      bounds: { width: 200, height: 300 },
    },
    nodes: [
      {
        id: "A",
        label: "Implementation",
        position: { x: 100, y: 50 },
        size: { width: 160, height: 40 },
      },
      {
        id: "B",
        label: "Quality Checks",
        position: { x: 100, y: 150 },
        size: { width: 160, height: 40 },
      },
    ],
    edges: [],
  };

  const defaultResult = convertToTldraw(mmds);
  const staticResult = convertToTldraw(mmds, { nodeSpacing: 1.2 });

  const geos = (result) =>
    result.records.filter((r) => r.typeName === "shape" && r.type === "geo");
  const yExtent = (shapes) => {
    const min = Math.min(...shapes.map((s) => s.y));
    const max = Math.max(...shapes.map((s) => s.y + s.props.h));
    return max - min;
  };

  const defaultExtent = yExtent(geos(defaultResult));
  const staticExtent = yExtent(geos(staticResult));

  assert.ok(
    defaultExtent > staticExtent,
    `Expected adaptive extent (${defaultExtent}) > static 1.2x extent (${staticExtent})`,
  );
});

test("explicit nodeSpacing overrides adaptive ratio", () => {
  const mmds = {
    version: 1,
    geometry_level: "layout",
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    metadata: {
      diagram_type: "flowchart",
      direction: "LR",
      bounds: { width: 400, height: 120 },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 50, y: 60 },
        size: { width: 80, height: 40 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 200, y: 60 },
        size: { width: 80, height: 40 },
      },
    ],
    edges: [],
  };

  const result_2x = convertToTldraw(mmds, { nodeSpacing: 2.0 });
  const result_1x = convertToTldraw(mmds, { nodeSpacing: 1.0 });

  const geos = (result) =>
    result.records.filter((r) => r.typeName === "shape" && r.type === "geo");
  const xExtent = (shapes) => {
    const min = Math.min(...shapes.map((s) => s.x));
    const max = Math.max(...shapes.map((s) => s.x + s.props.w));
    return max - min;
  };

  const extent_2x = xExtent(geos(result_2x));
  const extent_1x = xExtent(geos(result_1x));

  // Explicit 2.0x should produce wider spacing than 1.0x
  assert.ok(
    extent_2x > extent_1x,
    `Expected 2.0x extent (${extent_2x}) > 1.0x extent (${extent_1x})`,
  );
});

// Constants matching convert.ts
const CHAR_WIDTH_EST = 14;
const MIN_LABEL_PAD_X = 36;

function assertLabelsAllFit(converted, mmds, fixtureName) {
  const geos = converted.records.filter(
    (r) => r.typeName === "shape" && r.type === "geo",
  );
  const nodeById = new Map();
  for (const node of mmds.nodes) {
    nodeById.set(node.id, node);
  }

  for (const shape of geos) {
    // shape.id is like "shape:node_A" -> extract "A"
    const nodeId = String(shape.id).replace("shape:node_", "");
    const node = nodeById.get(nodeId);
    if (!node) continue;
    const label = node.label ?? node.id;
    const minW = label.length * CHAR_WIDTH_EST + MIN_LABEL_PAD_X;
    assert.ok(
      shape.props.w >= minW,
      `${fixtureName}: node ${nodeId} width (${shape.props.w}) < min for label "${label}" (${minW})`,
    );
  }
}

function assertNoOverlaps(converted, fixtureName) {
  const geos = converted.records.filter(
    (r) => r.typeName === "shape" && r.type === "geo",
  );
  // Group by parentId to only check siblings
  const byParent = new Map();
  for (const s of geos) {
    const pid = String(s.parentId);
    if (!byParent.has(pid)) byParent.set(pid, []);
    byParent.get(pid).push(s);
  }

  const MIN_GAP = 0; // Just check no actual overlap (gap >= 0)
  for (const [, siblings] of byParent) {
    for (let i = 0; i < siblings.length; i++) {
      for (let j = i + 1; j < siblings.length; j++) {
        const a = siblings[i];
        const b = siblings[j];
        const overlapX =
          a.x < b.x + b.props.w + MIN_GAP && b.x < a.x + a.props.w + MIN_GAP;
        const overlapY =
          a.y < b.y + b.props.h + MIN_GAP && b.y < a.y + a.props.h + MIN_GAP;
        assert.ok(
          !(overlapX && overlapY),
          `${fixtureName}: nodes ${a.id} and ${b.id} overlap`,
        );
      }
    }
  }
}

test("adaptive spacing fixture: layout-basic - labels fit and no overlaps", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "positioned",
    "layout-basic.json",
  );
  const converted = convertToTldraw(mmds);
  assertLabelsAllFit(converted, mmds, "layout-basic");
  assertNoOverlaps(converted, "layout-basic");
});

test("adaptive spacing fixture: routed-basic - labels fit and no overlaps", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "positioned",
    "routed-basic.json",
  );
  const converted = convertToTldraw(mmds);
  assertLabelsAllFit(converted, mmds, "routed-basic");
  assertNoOverlaps(converted, "routed-basic");
});

test("adaptive spacing fixture: layout-with-subgraphs - labels fit and no overlaps", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "layout-with-subgraphs.json",
  );
  const converted = convertToTldraw(mmds);
  assertLabelsAllFit(converted, mmds, "layout-with-subgraphs");
  assertNoOverlaps(converted, "layout-with-subgraphs");
});

test("adaptive spacing fixture: complex-roundtrip - labels fit and no overlaps", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "generation",
    "complex-roundtrip.json",
  );
  const converted = convertToTldraw(mmds);
  assertLabelsAllFit(converted, mmds, "complex-roundtrip");
  assertNoOverlaps(converted, "complex-roundtrip");
});

test("adaptive spacing fixture: shapes-and-strokes - labels fit and no overlaps", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "generation",
    "shapes-and-strokes.json",
  );
  const converted = convertToTldraw(mmds);
  assertLabelsAllFit(converted, mmds, "shapes-and-strokes");
  assertNoOverlaps(converted, "shapes-and-strokes");
});

// --- faceAndFractionToNormalizedAnchor ---

test("faceAndFractionToNormalizedAnchor: rectangle bottom center", () => {
  const result = faceAndFractionToNormalizedAnchor("bottom", 0.5, "rectangle");
  assert.deepEqual(result, { x: 0.5, y: 1.0 });
});

test("faceAndFractionToNormalizedAnchor: rectangle right 30%", () => {
  const result = faceAndFractionToNormalizedAnchor("right", 0.3, "rectangle");
  assert.deepEqual(result, { x: 1.0, y: 0.3 });
});

test("faceAndFractionToNormalizedAnchor: rectangle top left corner", () => {
  const result = faceAndFractionToNormalizedAnchor("top", 0.0, "rectangle");
  assert.deepEqual(result, { x: 0.0, y: 0.0 });
});

test("faceAndFractionToNormalizedAnchor: rectangle left bottom corner", () => {
  const result = faceAndFractionToNormalizedAnchor("left", 1.0, "rectangle");
  assert.deepEqual(result, { x: 0.0, y: 1.0 });
});

test("faceAndFractionToNormalizedAnchor: diamond right center projects to boundary", () => {
  const result = faceAndFractionToNormalizedAnchor("right", 0.5, "diamond");
  assert.ok(Math.abs(result.x - 1.0) < 0.1, `x=${result.x} should be near 1.0`);
  assert.ok(Math.abs(result.y - 0.5) < 0.1, `y=${result.y} should be near 0.5`);
});

test("faceAndFractionToNormalizedAnchor: hexagon bottom center", () => {
  const result = faceAndFractionToNormalizedAnchor("bottom", 0.5, "hexagon");
  assert.deepEqual(result, { x: 0.5, y: 1.0 });
});

test("faceAndFractionToNormalizedAnchor: clamps out-of-range fraction", () => {
  const lo = faceAndFractionToNormalizedAnchor("top", -0.5, "rectangle");
  assert.deepEqual(lo, { x: 0.0, y: 0.0 });
  const hi = faceAndFractionToNormalizedAnchor("top", 1.5, "rectangle");
  assert.deepEqual(hi, { x: 1.0, y: 0.0 });
});

// --- port-aware binding tests ---

test("binding prefers routed path anchors over ports when both are present", () => {
  const mmds = {
    version: 1,
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    geometry_level: "routed",
    metadata: {
      diagram_type: "flowchart",
      direction: "TD",
      bounds: { width: 100, height: 120 },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 50, y: 30 },
        size: { width: 40, height: 20 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 50, y: 90 },
        size: { width: 40, height: 20 },
      },
    ],
    edges: [
      {
        id: "e0",
        source: "A",
        target: "B",
        // Endpoints are intentionally inset from the corners while ports are at
        // face extremes; path anchors should win to avoid border collisions.
        path: [
          [34, 40],
          [34, 72],
          [66, 72],
          [66, 80],
        ],
        is_backward: false,
        source_port: {
          face: "bottom",
          fraction: 0.0,
          position: { x: 30, y: 40 },
          group_size: 1,
        },
        target_port: {
          face: "top",
          fraction: 1.0,
          position: { x: 70, y: 80 },
          group_size: 1,
        },
      },
    ],
  };
  const result = convertToTldraw(mmds);
  const bindings = result.records.filter((r) => r.typeName === "binding");
  const startBinding = bindings.find((b) => b.props.terminal === "start");
  const endBinding = bindings.find((b) => b.props.terminal === "end");

  assert.ok(startBinding, "should have start binding");
  assert.ok(endBinding, "should have end binding");
  // Path endpoints map to x≈0.1 (bottom) and x≈0.9 (top), not port fractions 0/1.
  assert.ok(Math.abs(startBinding.props.normalizedAnchor.x - 0.1) < 1e-6);
  assert.equal(startBinding.props.normalizedAnchor.y, 1.0);
  assert.ok(Math.abs(endBinding.props.normalizedAnchor.x - 0.9) < 1e-6);
  assert.equal(endBinding.props.normalizedAnchor.y, 0.0);
});

test("binding uses port metadata when routed path is missing", () => {
  const mmds = {
    version: 1,
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    geometry_level: "routed",
    metadata: {
      diagram_type: "flowchart",
      direction: "TD",
      bounds: { width: 100, height: 120 },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 50, y: 30 },
        size: { width: 40, height: 20 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 50, y: 90 },
        size: { width: 40, height: 20 },
      },
    ],
    edges: [
      {
        id: "e0",
        source: "A",
        target: "B",
        is_backward: false,
        source_port: {
          face: "bottom",
          fraction: 0.5,
          position: { x: 50, y: 40 },
          group_size: 1,
        },
        target_port: {
          face: "top",
          fraction: 0.5,
          position: { x: 50, y: 80 },
          group_size: 1,
        },
      },
    ],
  };
  const result = convertToTldraw(mmds);
  const bindings = result.records.filter((r) => r.typeName === "binding");
  const startBinding = bindings.find((b) => b.props.terminal === "start");
  const endBinding = bindings.find((b) => b.props.terminal === "end");

  assert.ok(startBinding, "should have start binding");
  assert.ok(endBinding, "should have end binding");
  assert.deepEqual(startBinding.props.normalizedAnchor, { x: 0.5, y: 1.0 });
  assert.deepEqual(endBinding.props.normalizedAnchor, { x: 0.5, y: 0.0 });
});

test("binding falls back to edgeAnchor when no ports", () => {
  const mmds = {
    version: 1,
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    geometry_level: "routed",
    metadata: {
      diagram_type: "flowchart",
      direction: "TD",
      bounds: { width: 100, height: 120 },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 50, y: 30 },
        size: { width: 40, height: 20 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 50, y: 90 },
        size: { width: 40, height: 20 },
      },
    ],
    edges: [
      {
        id: "e0",
        source: "A",
        target: "B",
        path: [
          [50, 40],
          [50, 80],
        ],
        is_backward: false,
        // No source_port or target_port - should use edgeAnchor fallback
      },
    ],
  };
  const result = convertToTldraw(mmds);
  const bindings = result.records.filter((r) => r.typeName === "binding");
  const startBinding = bindings.find((b) => b.props.terminal === "start");
  assert.ok(startBinding, "should have start binding");
  // edgeAnchor should produce some valid normalizedAnchor
  assert.ok(typeof startBinding.props.normalizedAnchor.x === "number");
  assert.ok(typeof startBinding.props.normalizedAnchor.y === "number");
});

test("vertical elbows derive elbowMidPoint from the routed horizontal lane", () => {
  const mmds = {
    version: 1,
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    geometry_level: "routed",
    metadata: {
      diagram_type: "flowchart",
      direction: "TD",
      bounds: { width: 240, height: 280 },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 100, y: 30 },
        size: { width: 80, height: 40 },
      },
      {
        id: "C",
        label: "C",
        position: { x: 100, y: 240 },
        size: { width: 80, height: 40 },
      },
    ],
    edges: [
      {
        id: "e0",
        source: "A",
        target: "C",
        path: [
          [140, 50],
          [140, 210],
          [100, 210],
          [100, 220],
        ],
        source_port: {
          face: "bottom",
          fraction: 1,
          position: { x: 140, y: 50 },
          group_size: 1,
        },
        target_port: {
          face: "top",
          fraction: 0.5,
          position: { x: 100, y: 220 },
          group_size: 1,
        },
      },
    ],
  };

  const result = convertToTldraw(mmds);
  const edge = result.records.find(
    (record) =>
      record.typeName === "shape" &&
      record.type === "arrow" &&
      record.id === "shape:edge_e0",
  );
  assert.ok(edge);
  assert.ok(
    edge.props.elbowMidPoint > 0.9,
    `expected elbowMidPoint to follow the routed lane near target, got ${edge.props.elbowMidPoint}`,
  );
});

test("vertical elbows nudge away from intermediate node borders", () => {
  const mmds = {
    version: 1,
    defaults: {
      node: { shape: "rectangle" },
      edge: {
        stroke: "solid",
        arrow_start: "none",
        arrow_end: "normal",
        minlen: 1,
      },
    },
    geometry_level: "routed",
    metadata: {
      diagram_type: "flowchart",
      direction: "TD",
      bounds: { width: 260, height: 300 },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 100, y: 30 },
        size: { width: 80, height: 40 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 120, y: 140 },
        size: { width: 80, height: 40 },
      },
      {
        id: "C",
        label: "C",
        position: { x: 100, y: 240 },
        size: { width: 80, height: 40 },
      },
    ],
    edges: [
      {
        id: "e0",
        source: "A",
        target: "C",
        // The horizontal lane sits exactly on B's bottom border (y=160).
        path: [
          [140, 50],
          [140, 160],
          [100, 160],
          [100, 220],
        ],
        source_port: {
          face: "bottom",
          fraction: 1,
          position: { x: 140, y: 50 },
          group_size: 1,
        },
        target_port: {
          face: "top",
          fraction: 0.5,
          position: { x: 100, y: 220 },
          group_size: 1,
        },
      },
    ],
  };

  const result = convertToTldraw(mmds);
  const edge = result.records.find(
    (record) =>
      record.typeName === "shape" &&
      record.type === "arrow" &&
      record.id === "shape:edge_e0",
  );
  assert.ok(edge);
  assert.ok(
    edge.props.elbowMidPoint > 0.66,
    `expected elbowMidPoint to be nudged away from collision, got ${edge.props.elbowMidPoint}`,
  );
});

// --- Phase 8: Integration & backward compat ---

test("full pipeline: fan-in edges have distinct normalizedAnchors", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "positioned",
    "routed-fan-in-ports.json",
  );
  const result = convertToTldraw(mmds);
  const bindings = result.records.filter((r) => r.typeName === "binding");
  const endBindings = bindings.filter((b) => b.props.terminal === "end");
  assert.equal(
    endBindings.length,
    3,
    "should have 3 end bindings (one per edge)",
  );

  const anchorsX = endBindings.map((b) => b.props.normalizedAnchor.x);
  const unique = new Set(anchorsX.map((x) => x.toFixed(4)));
  assert.equal(
    unique.size,
    3,
    `fan-in edges should have 3 distinct anchor x-values, got: ${[...unique]}`,
  );

  // Validate the tldraw file parses correctly
  const file = toTldrawFile(mmds);
  assertParses(file);
});

test("backward compat: layout-level MMDS without ports converts successfully", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "positioned",
    "layout-basic.json",
  );
  const result = convertToTldraw(mmds);
  assert.ok(result.records.length > 0, "should produce records");
  const bindings = result.records.filter((r) => r.typeName === "binding");
  assert.ok(bindings.length > 0, "should have bindings (edgeAnchor fallback)");
});

test("backward compat: old MMDS fixture without port fields normalizes correctly", () => {
  const raw = fs.readFileSync(
    path.join(
      repoRoot,
      "tests",
      "fixtures",
      "mmds",
      "positioned",
      "layout-basic.json",
    ),
    "utf-8",
  );
  const doc = JSON.parse(raw);
  const normalized = normalizeMmds(doc);
  for (const edge of normalized.edges) {
    assert.equal(
      edge.source_port,
      undefined,
      `edge ${edge.id} should have no source_port`,
    );
    assert.equal(
      edge.target_port,
      undefined,
      `edge ${edge.id} should have no target_port`,
    );
  }
  // Convert should succeed
  const result = convertToTldraw(doc);
  assert.ok(result.records.length > 0);
});

test("routed fixture with ports: all bindings remain precise", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "positioned",
    "routed-fan-in-ports.json",
  );
  const result = convertToTldraw(mmds);
  const bindings = result.records.filter((r) => r.typeName === "binding");
  assert.ok(bindings.length > 0, "should have bindings");
  // All bindings should have isPrecise: true
  for (const binding of bindings) {
    assert.equal(
      binding.props.isPrecise,
      true,
      `binding ${binding.id} should be precise`,
    );
  }
});

// --- Snapshot tests ---

const SNAPSHOT_FIXTURES = [
  ["positioned", "layout-basic.json"],
  ["positioned", "routed-basic.json"],
  ["positioned", "routed-fan-in-ports.json"],
  ["layout-with-subgraphs.json"],
  ["generation", "shapes-and-strokes.json"],
];

const snapshotDir = path.join(import.meta.dirname, "snapshots");
const regenerateSnapshots = !!process.env.GENERATE_TLDRAW_SNAPSHOTS;

for (const segments of SNAPSHOT_FIXTURES) {
  const name = segments.at(-1).replace(".json", "");
  test(`snapshot: ${name}`, () => {
    const mmds = fixture("tests", "fixtures", "mmds", ...segments);
    const { records } = convertToTldraw(mmds);
    const json = `${JSON.stringify(records, null, 2)}\n`;
    const snapPath = path.join(snapshotDir, `${name}.snap.json`);

    if (regenerateSnapshots) {
      fs.mkdirSync(snapshotDir, { recursive: true });
      fs.writeFileSync(snapPath, json);
    } else {
      const expected = fs.readFileSync(snapPath, "utf-8");
      assert.equal(
        json,
        expected,
        `Snapshot mismatch: ${name}. Run GENERATE_TLDRAW_SNAPSHOTS=1 to update.`,
      );
    }
  });
}

test("deterministic ordering: same MMDS produces identical tldraw output", () => {
  const mmds = fixture(
    "tests",
    "fixtures",
    "mmds",
    "subgraph-endpoint-intent-present.json",
  );

  const a = toTldrawFile(mmds);
  const b = toTldrawFile(mmds);

  assert.deepEqual(a, b);

  const storeA = convertToTldrawStore(mmds);
  const storeB = convertToTldrawStore(mmds);
  assert.deepEqual(storeA, storeB);
});
