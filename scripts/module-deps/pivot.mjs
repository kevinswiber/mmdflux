#!/usr/bin/env node
// Generate a pivoted Mermaid dependency view for Rust modules.

import { parseArgs } from 'node:util';
import { existsSync } from 'node:fs';
import {
  discoverModules, collectDependencies, collapseGraph,
  tarjanScc, buildCondensationGraph, reverseEdges, writeOutput,
} from './core.mjs';

const { values } = parseArgs({
  options: {
    'crate-root': { type: 'string', default: 'src/lib.rs' },
    module: { type: 'string' },
    'max-depth': { type: 'string', default: '1' },
    direction: { type: 'string', default: 'outbound' },
    mode: { type: 'string', default: 'tree' },
    'condense-scc': { type: 'boolean', default: false },
    'direction-layout': { type: 'string', default: 'LR' },
    'members-per-line': { type: 'string', default: '3' },
    output: { type: 'string' },
    help: { type: 'boolean', short: 'h' },
  },
  strict: true,
});

if (values.help) {
  console.log(`Usage: pivot.mjs --module <name> [options]

Options:
  --module <name>            Module to pivot on (required)
  --crate-root <path>        Crate root file (default: src/lib.rs)
  --max-depth <n>            Collapse depth (default: 1)
  --direction <dir>          outbound, inbound, or both (default: outbound)
  --mode <mode>              tree or dag (default: tree)
  --condense-scc             Operate on SCC-condensed graph
  --direction-layout <dir>   LR, RL, TB, or BT (default: LR)
  --members-per-line <n>     SCC label line width (default: 3)
  --output <path>            Write to file instead of stdout
  -h, --help                 Show this help`);
  process.exit(0);
}

const crateRoot = values['crate-root'];
const requestedModule = values.module;
const maxDepth = Number(values['max-depth']);
const direction = values.direction;
const mode = values.mode;
const condenseScc = values['condense-scc'];
const directionLayout = values['direction-layout'];
const membersPerLine = Number(values['members-per-line']);

if (!requestedModule) { console.error('--module is required'); process.exit(2); }
if (maxDepth < 1) { console.error('--max-depth must be >= 1'); process.exit(2); }
if (membersPerLine < 1) { console.error('--members-per-line must be >= 1'); process.exit(2); }
if (!['outbound', 'inbound', 'both'].includes(direction)) {
  console.error('--direction must be outbound, inbound, or both'); process.exit(2);
}
if (!['tree', 'dag'].includes(mode)) {
  console.error('--mode must be tree or dag'); process.exit(2);
}
if (mode === 'tree' && direction === 'both') {
  console.error('--mode tree requires --direction inbound or outbound'); process.exit(2);
}
if (!['LR', 'RL', 'TB', 'BT'].includes(directionLayout)) {
  console.error('--direction-layout must be LR, RL, TB, or BT'); process.exit(2);
}
if (!existsSync(crateRoot)) {
  console.error(`crate root does not exist: ${crateRoot}`); process.exit(2);
}

// --- Helpers ---

function nodeId(label) {
  return 'node_' + (label.replace(/[^A-Za-z0-9_]+/g, '_').replace(/^_|_$/g, '') || 'root');
}

function sccLabel(component) {
  if (component.length === 1) return component[0];
  const labelLines = [`cycle (${component.length} modules)`];
  for (let i = 0; i < component.length; i += membersPerLine) {
    labelLines.push(component.slice(i, i + membersPerLine).join(', '));
  }
  return labelLines.join('<br/>');
}

function buildRenderGraph(nodes, edges) {
  const rawToRender = new Map();
  const displayLabels = new Map();
  const renderEdges = new Map();
  const nodeStyles = new Map();
  const renderNodeIds = new Map();

  if (!condenseScc) {
    for (const label of nodes) {
      rawToRender.set(label, label);
      displayLabels.set(label, label);
      renderEdges.set(label, new Set(edges.get(label) || []));
      nodeStyles.set(label, 'normal');
      renderNodeIds.set(label, nodeId(label));
    }
    return { rawToRender, displayLabels, renderEdges, nodeStyles, nodeIds: renderNodeIds };
  }

  const components = tarjanScc(nodes, edges);
  const { componentNodes, condensationEdges, nodeToComponent } =
    buildCondensationGraph(components, edges);

  for (const [cid, members] of componentNodes) {
    const key = `scc::${cid}`;
    displayLabels.set(key, sccLabel(members));
    nodeStyles.set(key, members.length > 1 ? 'cycle' : 'normal');
    renderNodeIds.set(key, nodeId(key));
  }

  for (const [cid, targets] of condensationEdges) {
    const key = `scc::${cid}`;
    renderEdges.set(key, new Set([...targets].map(t => `scc::${t}`)));
  }

  for (const node of nodes) {
    rawToRender.set(node, `scc::${nodeToComponent.get(node)}`);
  }

  return { rawToRender, displayLabels, renderEdges, nodeStyles, nodeIds: renderNodeIds };
}

function treeEdgesFromPivot(pivot, edges) {
  const traversal = direction === 'outbound' ? edges : reverseEdges(edges);
  const visited = new Set([pivot]);
  const pairs = [];
  const queue = [pivot];
  let head = 0;

  while (head < queue.length) {
    const current = queue[head++];
    for (const neighbor of [...(traversal.get(current) || [])].sort()) {
      if (visited.has(neighbor)) continue;
      visited.add(neighbor);
      queue.push(neighbor);
      pairs.push(direction === 'outbound' ? [current, neighbor] : [neighbor, current]);
    }
  }
  return { nodes: visited, edges: pairs };
}

function reachableNodes(pivot, edges) {
  function walk(start, graph) {
    const seen = new Set([start]);
    const queue = [start];
    let head = 0;
    while (head < queue.length) {
      const current = queue[head++];
      for (const neighbor of [...(graph.get(current) || [])].sort()) {
        if (!seen.has(neighbor)) { seen.add(neighbor); queue.push(neighbor); }
      }
    }
    return seen;
  }

  if (direction === 'outbound') return walk(pivot, edges);
  if (direction === 'inbound') return walk(pivot, reverseEdges(edges));
  const fwd = walk(pivot, edges);
  const bwd = walk(pivot, reverseEdges(edges));
  return new Set([...fwd, ...bwd]);
}

function dagSubgraphEdges(included, edges) {
  const pairs = [];
  for (const [source, targets] of edges) {
    if (!included.has(source)) continue;
    for (const target of targets) {
      if (included.has(target)) pairs.push([source, target]);
    }
  }
  return pairs;
}

// --- Main ---

const modules = discoverModules(crateRoot);
const dependencies = collectDependencies(modules, { stripTestItems: false });
const { nodes: rawNodes, edges: rawEdges } = collapseGraph(modules, dependencies, maxDepth);
const { rawToRender, displayLabels, renderEdges, nodeStyles, nodeIds } =
  buildRenderGraph(rawNodes, rawEdges);

// Resolve pivot
const normalized = requestedModule.trim();
if (!rawToRender.has(normalized)) {
  const available = [...rawNodes].sort().join(', ');
  console.error(
    `unknown module '${requestedModule}' for this max-depth. Available modules: ${available}`,
  );
  process.exit(2);
}
const pivotLabel = rawToRender.get(normalized);

// Compute included nodes and edges
let includedNodes, edgePairs;
if (mode === 'tree') {
  const result = treeEdgesFromPivot(pivotLabel, renderEdges);
  includedNodes = result.nodes;
  edgePairs = result.edges;
} else {
  includedNodes = reachableNodes(pivotLabel, renderEdges);
  edgePairs = dagSubgraphEdges(includedNodes, renderEdges);
}

edgePairs.sort((a, b) =>
  a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : (a[1] < b[1] ? -1 : a[1] > b[1] ? 1 : 0));

// Render
const lines = [
  '%% Generated by scripts/module-deps/pivot.mjs',
  `%% crate-root: ${crateRoot}`,
  `%% max-depth: ${maxDepth}`,
  `%% pivot-request: ${requestedModule}`,
  `%% pivot-node: ${pivotLabel}`,
  `%% mode: ${mode}`,
  `%% direction: ${direction}`,
  `%% condensed-scc: ${condenseScc ? 'yes' : 'no'}`,
  `flowchart ${directionLayout}`,
];

for (const node of [...includedNodes].sort()) {
  lines.push(`    ${nodeIds.get(node)}["${displayLabels.get(node)}"]`);
}

for (const [source, target] of edgePairs) {
  lines.push(`    ${nodeIds.get(source)} --> ${nodeIds.get(target)}`);
}

lines.push(
  '    classDef pivot fill:#fde68a,stroke:#b45309,stroke-width:2px,color:#1f2937;',
  '    classDef cycle fill:#fbe4e6,stroke:#b42318,stroke-width:2px,color:#1f2937;',
  '    classDef normal fill:#e8f0ff,stroke:#4f46e5,stroke-width:1.5px,color:#1f2937;',
);

lines.push(`    class ${nodeIds.get(pivotLabel)} pivot;`);

const cycleNodes = [...nodeStyles]
  .filter(([n, s]) => s === 'cycle' && includedNodes.has(n) && n !== pivotLabel)
  .map(([n]) => n);
const normalNodes = [...nodeStyles]
  .filter(([n, s]) => s === 'normal' && includedNodes.has(n) && n !== pivotLabel)
  .map(([n]) => n);

if (cycleNodes.length) {
  lines.push('    class ' + cycleNodes.sort().map(n => nodeIds.get(n)).join(',') + ' cycle;');
}
if (normalNodes.length) {
  lines.push('    class ' + normalNodes.sort().map(n => nodeIds.get(n)).join(',') + ' normal;');
}

const diagram = lines.join('\n') + '\n';
writeOutput(diagram, values.output || null);
