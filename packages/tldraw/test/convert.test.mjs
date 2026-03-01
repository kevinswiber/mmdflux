import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

import { createTLStore, parseTldrawJsonFile } from "tldraw";

import {
  convertToTldraw,
  convertToTldrawStore,
  toTldrawFile,
} from "../dist/convert.js";

const repoRoot = path.resolve(process.cwd(), "../..");

function fixture(...segments) {
  const fullPath = path.join(repoRoot, ...segments);
  return JSON.parse(fs.readFileSync(fullPath, "utf8"));
}

function assertParses(file) {
  const schema = createTLStore().schema;
  const parsed = parseTldrawJsonFile({
    json: JSON.stringify(file),
    schema,
  });

  assert.equal(parsed.ok, true);
}

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
