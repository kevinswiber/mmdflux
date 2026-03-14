import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

import {
  collectDescendantSubgraphIds,
  collectNodeAncestorSubgraphIds,
  collectSubgraphAncestorIds,
  collectSubgraphDescendantNodeIds,
  edgeEndpointTargets,
  normalizeMmds,
} from "../dist/index.js";

const repoRoot = path.resolve(process.cwd(), "../..");

function fixture(...segments) {
  const fullPath = path.join(repoRoot, ...segments);
  return JSON.parse(fs.readFileSync(fullPath, "utf8"));
}

test("mmds-core exposes a curated top-level surface and explicit subpath modules", async () => {
  const core = await import("@mmds/core");
  assert.ok("normalizeMmds" in core);
  assert.ok("collectSubgraphDescendantNodeIds" in core);
  assert.equal("assertValidMmdsDocument" in core, false);
  assert.equal("normalizePath" in core, false);

  const normalize = await import("@mmds/core/normalize");
  assert.ok("normalizeMmds" in normalize);
  assert.equal("collectSubgraphDescendantNodeIds" in normalize, false);

  const traversal = await import("@mmds/core/traversal");
  assert.ok("collectSubgraphDescendantNodeIds" in traversal);
  assert.ok("edgeEndpointTargets" in traversal);
  assert.equal("normalizeMmds" in traversal, false);

  const validate = await import("@mmds/core/validate");
  assert.ok("assertValidMmdsDocument" in validate);
  assert.equal("normalizeMmds" in validate, false);

  await import("@mmds/core/types");
});

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

test("normalizeMmds passes through edge port metadata", () => {
  const doc = {
    version: 1,
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 0, y: 0 },
        size: { width: 40, height: 20 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 0, y: 50 },
        size: { width: 40, height: 20 },
      },
    ],
    edges: [
      {
        id: "e0",
        source: "A",
        target: "B",
        source_port: {
          face: "bottom",
          fraction: 0.5,
          position: { x: 50, y: 35 },
          group_size: 1,
        },
        target_port: {
          face: "top",
          fraction: 0.5,
          position: { x: 50, y: 40 },
          group_size: 1,
        },
      },
    ],
  };
  const normalized = normalizeMmds(doc);
  assert.deepEqual(normalized.edges[0].source_port, {
    face: "bottom",
    fraction: 0.5,
    position: { x: 50, y: 35 },
    group_size: 1,
  });
  assert.deepEqual(normalized.edges[0].target_port, {
    face: "top",
    fraction: 0.5,
    position: { x: 50, y: 40 },
    group_size: 1,
  });
});

test("normalizeMmds sets port to undefined when absent", () => {
  const doc = {
    version: 1,
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 0, y: 0 },
        size: { width: 40, height: 20 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 0, y: 50 },
        size: { width: 40, height: 20 },
      },
    ],
    edges: [{ id: "e0", source: "A", target: "B" }],
  };
  const normalized = normalizeMmds(doc);
  assert.equal(normalized.edges[0].source_port, undefined);
  assert.equal(normalized.edges[0].target_port, undefined);
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

test("normalizeMmds preserves shared profile fixtures from the Rust contract set", () => {
  const doc = fixture(
    "tests",
    "fixtures",
    "mmds",
    "profiles",
    "profiles-svg-v1.json",
  );

  const normalized = normalizeMmds(doc);

  assert.deepEqual(normalized.profiles, ["mmds-core-v1", "mmdflux-svg-v1"]);
  assert.deepEqual(normalized.extensions, doc.extensions);
});

test("normalizeMmds consumes the locked shared flowchart contract fixture", () => {
  const doc = fixture(
    "tests",
    "fixtures",
    "mmds",
    "contracts",
    "flowchart-simple.layout.json",
  );

  const normalized = normalizeMmds(doc);

  assert.equal(normalized.metadata?.diagram_type, "flowchart");
  assert.deepEqual(
    normalized.nodes.map((node) => node.id),
    ["A", "B"],
  );
  assert.equal(normalized.edges[0]?.source, "A");
  assert.equal(normalized.edges[0]?.target, "B");
});
