import assert from "node:assert/strict";
import test from "node:test";

import { convert } from "../dist/convert.js";

function minimalDoc(overrides = {}) {
  return {
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
    geometry_level: "layout",
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
    edges: [{ id: "e0", source: "A", target: "B" }],
    ...overrides,
  };
}

test("maps double_circle shape to ellipse", () => {
  const doc = minimalDoc({
    nodes: [
      {
        id: "A",
        label: "A",
        shape: "double_circle",
        position: { x: 20, y: 20 },
        size: { width: 40, height: 20 },
      },
    ],
    edges: [],
  });

  const { elements } = convert(doc);
  const nodeShape = elements.find((e) => e.id === "A");
  assert.ok(nodeShape);
  assert.equal(nodeShape.type, "ellipse");
});

test("skips invisible edges", () => {
  const doc = minimalDoc({
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
      },
    ],
  });

  const { elements } = convert(doc);
  const arrows = elements.filter((e) => e.type === "arrow");
  assert.equal(arrows.length, 1);
  assert.equal(arrows[0].id, "e1");
});

test("uses parent chain for nested subgraph group ids", () => {
  const doc = minimalDoc({
    nodes: [
      {
        id: "A",
        label: "A",
        parent: "child",
        position: { x: 20, y: 20 },
        size: { width: 40, height: 20 },
      },
    ],
    edges: [],
    subgraphs: [
      { id: "root", title: "root", children: [] },
      { id: "child", title: "child", parent: "root", children: ["A"] },
    ],
  });

  const { elements } = convert(doc);
  const shape = elements.find((e) => e.id === "A");
  assert.ok(shape);
  assert.deepEqual(shape.groupIds, ["group_child", "group_root"]);
});

test("does not bind subgraph-endpoint arrows to node centers", () => {
  const doc = minimalDoc({
    edges: [
      {
        id: "e0",
        source: "A",
        target: "B",
        from_subgraph: "sg1",
        to_subgraph: "sg2",
        path: [
          [20, 20],
          [80, 20],
          [120, 20],
        ],
      },
    ],
  });

  const { elements } = convert(doc);
  const arrow = elements.find((e) => e.id === "e0" && e.type === "arrow");
  assert.ok(arrow);
  assert.equal("startBinding" in arrow, false);
  assert.equal("endBinding" in arrow, false);
});
