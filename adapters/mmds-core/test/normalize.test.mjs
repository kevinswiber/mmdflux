import assert from "node:assert/strict";
import test from "node:test";

import {
  collectDescendantSubgraphIds,
  collectNodeAncestorSubgraphIds,
  collectSubgraphAncestorIds,
  collectSubgraphDescendantNodeIds,
  edgeEndpointTargets,
  normalizeMmds,
} from "../dist/index.js";

test("normalizeMmds expands defaults for nodes and edges", () => {
  const doc = {
    version: 1,
    defaults: {
      node: { shape: "round" },
      edge: {
        stroke: "dotted",
        arrow_start: "circle",
        arrow_end: "cross",
        minlen: 2,
      },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 0, y: 0 },
        size: { width: 10, height: 10 },
      },
    ],
    edges: [{ id: "e0", source: "A", target: "A" }],
  };

  const normalized = normalizeMmds(doc);
  assert.equal(normalized.nodes[0].shape, "round");
  assert.equal(normalized.edges[0].stroke, "dotted");
  assert.equal(normalized.edges[0].arrow_start, "circle");
  assert.equal(normalized.edges[0].arrow_end, "cross");
  assert.equal(normalized.edges[0].minlen, 2);
});

test("subgraph traversal helpers walk parent and descendant chains", () => {
  const doc = {
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
    nodes: [
      {
        id: "N0",
        label: "N0",
        parent: "sg_leaf",
        position: { x: 0, y: 0 },
        size: { width: 10, height: 10 },
      },
      {
        id: "N1",
        label: "N1",
        parent: "sg_mid",
        position: { x: 1, y: 1 },
        size: { width: 10, height: 10 },
      },
    ],
    edges: [],
    subgraphs: [
      { id: "sg_root", title: "root", children: ["N1"] },
      {
        id: "sg_mid",
        title: "mid",
        parent: "sg_root",
        children: ["N1"],
      },
      {
        id: "sg_leaf",
        title: "leaf",
        parent: "sg_mid",
        children: ["N0"],
      },
    ],
  };

  const normalized = normalizeMmds(doc);

  assert.deepEqual(
    collectSubgraphAncestorIds("sg_leaf", normalized.subgraph_by_id),
    ["sg_mid", "sg_root"],
  );

  assert.deepEqual(
    collectNodeAncestorSubgraphIds(
      normalized.nodes[0],
      normalized.subgraph_by_id,
    ),
    ["sg_leaf", "sg_mid", "sg_root"],
  );

  assert.deepEqual(
    collectDescendantSubgraphIds("sg_root", normalized.subgraphs),
    ["sg_mid", "sg_leaf"],
  );

  assert.deepEqual(
    collectSubgraphDescendantNodeIds(
      "sg_root",
      normalized.nodes,
      normalized.subgraphs,
    ),
    ["N0", "N1"],
  );
});

test("edgeEndpointTargets resolves endpoint intent to node or subgraph targets", () => {
  const plain = edgeEndpointTargets({
    source: "A",
    target: "B",
  });
  assert.deepEqual(plain, {
    from: {
      kind: "node",
      id: "A",
      node_id: "A",
      subgraph_id: undefined,
    },
    to: {
      kind: "node",
      id: "B",
      node_id: "B",
      subgraph_id: undefined,
    },
  });

  const subgraphIntent = edgeEndpointTargets({
    source: "A",
    target: "B",
    from_subgraph: "sg1",
    to_subgraph: "sg2",
  });
  assert.deepEqual(subgraphIntent, {
    from: {
      kind: "subgraph",
      id: "sg1",
      node_id: "A",
      subgraph_id: "sg1",
    },
    to: {
      kind: "subgraph",
      id: "sg2",
      node_id: "B",
      subgraph_id: "sg2",
    },
  });
});
