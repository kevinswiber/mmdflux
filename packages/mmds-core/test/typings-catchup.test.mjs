import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

import {
  getExtension,
  getTextMetricsExtension,
  MMDS_NODE_STYLE_NAMESPACE,
  MMDS_NODE_STYLE_PROFILE,
  MMDS_TEXT_MEASUREMENTS_NAMESPACE,
  MMDS_TEXT_MEASUREMENTS_PROFILE,
  MMDS_TEXT_METRICS_NAMESPACE,
  MMDS_TEXT_METRICS_PROFILE,
} from "../dist/extensions.js";
import { normalizeMmds } from "../dist/index.js";

const repoRoot = path.resolve(process.cwd(), "../..");

function fixture(...segments) {
  const fullPath = path.join(repoRoot, ...segments);
  return JSON.parse(fs.readFileSync(fullPath, "utf8"));
}

test("typings-catchup harness is wired", () => {
  assert.ok(true);
});

test("node-style namespace constants match schema literals", () => {
  assert.equal(MMDS_NODE_STYLE_NAMESPACE, "org.mmdflux.node-style.v1");
  assert.equal(MMDS_NODE_STYLE_PROFILE, "mmdflux-node-style-v1");
});

test("text metrics helpers read the shared flowchart contract fixture", () => {
  assert.equal(MMDS_TEXT_METRICS_NAMESPACE, "org.mmdflux.text-metrics.v1");
  assert.equal(MMDS_TEXT_METRICS_PROFILE, "mmdflux-text-metrics-v1");
  assert.equal(
    MMDS_TEXT_MEASUREMENTS_NAMESPACE,
    "org.mmdflux.text-measurements.v1",
  );
  assert.equal(MMDS_TEXT_MEASUREMENTS_PROFILE, "mmdflux-text-measurements-v1");

  const doc = fixture(
    "tests",
    "fixtures",
    "mmds",
    "contracts",
    "flowchart-style.layout.json",
  );
  const textMetrics = getTextMetricsExtension(doc);
  assert.equal(textMetrics?.metricsProfile.id, "mmdflux-sans-v1");

  const genericTextMetrics = getExtension(doc, MMDS_TEXT_METRICS_NAMESPACE);
  assert.equal(genericTextMetrics?.metricsProfile.id, "mmdflux-sans-v1");
});

test("normalizeMmds passes label_side and label_rect through unchanged", () => {
  const doc = {
    version: 1,
    geometry_level: "routed",
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 0, y: 0 },
        size: { width: 10, height: 10 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 20, y: 0 },
        size: { width: 10, height: 10 },
      },
    ],
    edges: [
      {
        id: "e0",
        source: "A",
        target: "B",
        label: "yes",
        label_side: "above",
        label_rect: { x: 5, y: 5, width: 8, height: 4 },
      },
      { id: "e1", source: "B", target: "A" },
    ],
  };

  const out = normalizeMmds(doc);
  assert.equal(out.edges[0].label_side, "above");
  assert.deepEqual(out.edges[0].label_rect, {
    x: 5,
    y: 5,
    width: 8,
    height: 4,
  });
  assert.equal(out.edges[1].label_side, undefined);
  assert.equal(out.edges[1].label_rect, undefined);
});

test("metadata engine and diagnostics round-trip", () => {
  const routedFixture = fixture(
    "tests",
    "fixtures",
    "mmds",
    "positioned",
    "routed-fan-in-ports.json",
  );
  const routedOut = normalizeMmds(routedFixture);
  assert.equal(routedOut.metadata?.engine, "flux-layered");

  const diagnosticsDoc = {
    version: 1,
    geometry_level: "routed",
    metadata: {
      diagram_type: "flowchart",
      direction: "TD",
      engine: "flux-layered",
      diagnostics: {
        unfit_label_overlaps: [
          {
            edge_id: "e0",
            label: "yes",
            gap_pixels: 12,
            label_span_pixels: 30,
            attempted_side: "above",
          },
        ],
      },
    },
    nodes: [],
    edges: [],
  };
  const diagnosticsOut = normalizeMmds(diagnosticsDoc);
  assert.equal(diagnosticsOut.metadata?.engine, "flux-layered");
  assert.equal(
    diagnosticsOut.metadata?.diagnostics?.unfit_label_overlaps?.[0]?.edge_id,
    "e0",
  );
  assert.equal(
    diagnosticsOut.metadata?.diagnostics?.unfit_label_overlaps?.[0]
      ?.attempted_side,
    "above",
  );
});

test("normalizeMmds preserves subgraph concurrent_regions when present", () => {
  const doc = {
    version: 1,
    metadata: { diagram_type: "state" },
    nodes: [],
    edges: [],
    subgraphs: [
      {
        id: "sg_state",
        title: "State",
        children: [],
        concurrent_regions: ["fork_top", 42, "fork_bottom"],
      },
      {
        id: "sg_plain",
        title: "Plain",
        children: [],
      },
      {
        id: "sg_empty",
        title: "Empty",
        children: [],
        concurrent_regions: [],
      },
    ],
  };

  const out = normalizeMmds(doc);
  assert.deepEqual(out.subgraphs[0].concurrent_regions, [
    "fork_top",
    "fork_bottom",
  ]);
  assert.equal(out.subgraphs[1].concurrent_regions, undefined);
  assert.equal(out.subgraphs[2].concurrent_regions, undefined);
});

test("normalizeMmds extracts classNames from the node-style extension", () => {
  const doc = {
    version: 1,
    extensions: {
      "org.mmdflux.node-style.v1": {
        nodes: {
          A: { classNames: ["alpha", 42, "beta"] },
          B: { classNames: [] },
        },
        subgraphs: {
          sg1: { classNames: ["root-class"] },
          sg2: { classNames: [] },
        },
      },
    },
    nodes: [
      {
        id: "A",
        label: "A",
        position: { x: 0, y: 0 },
        size: { width: 10, height: 10 },
      },
      {
        id: "B",
        label: "B",
        position: { x: 20, y: 0 },
        size: { width: 10, height: 10 },
      },
    ],
    edges: [],
    subgraphs: [
      { id: "sg1", title: "Root", children: ["A"] },
      { id: "sg2", title: "Empty", children: [] },
    ],
  };

  const out = normalizeMmds(doc);
  assert.deepEqual(out.nodes.find((node) => node.id === "A")?.classNames, [
    "alpha",
    "beta",
  ]);
  assert.equal(
    out.nodes.find((node) => node.id === "B")?.classNames,
    undefined,
  );
  assert.deepEqual(out.subgraphs[0].classNames, ["root-class"]);
  assert.equal(out.subgraphs[1].classNames, undefined);
});

test("normalizeMmds leaves classNames undefined when the extension is absent", () => {
  const styleFixture = fixture(
    "tests",
    "fixtures",
    "mmds",
    "contracts",
    "flowchart-style.layout.json",
  );
  const styleOut = normalizeMmds(styleFixture);
  assert.equal(styleOut.nodes[0].classNames, undefined);

  const simpleFixture = fixture(
    "tests",
    "fixtures",
    "mmds",
    "contracts",
    "flowchart-simple.layout.json",
  );
  const simpleOut = normalizeMmds(simpleFixture);
  assert.ok(simpleOut.nodes.every((node) => node.classNames === undefined));
  assert.ok(
    simpleOut.subgraphs.every((subgraph) => subgraph.classNames === undefined),
  );
});
