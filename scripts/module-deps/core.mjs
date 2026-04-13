// Shared utilities for Rust module dependency analysis.
//
// Used by the individual scripts in this directory:
// - flowchart.mjs  (just module-map)
// - c4.mjs         (just module-map-c4)
// - scc.mjs        (just module-map-scc)
// - pivot.mjs      (just module-map-{outbound,inbound,pivot-dag})

import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { join, dirname, basename, extname, resolve } from 'node:path';

// --- Regex patterns (match the Python originals) ---

const MOD_DECL_RE = /^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;\s*$/;
const USE_RE = /\b(?:pub\s+)?use\s+([^;]+);/g;
const PATH_TOKEN_RE = /\b(?:crate|self|super)(?:::[A-Za-z_][A-Za-z0-9_]*)+/g;
const BLOCK_COMMENT_RE = /\/\*[\s\S]*?\*\//g;
const LINE_COMMENT_RE = /\/\/.*$/gm;

// --- Internal helpers ---

function countChar(s, ch) {
  let n = 0;
  for (let i = 0; i < s.length; i++) if (s[i] === ch) n++;
  return n;
}

function moduleDirFor(filePath) {
  const name = basename(filePath);
  if (name === 'lib.rs' || name === 'main.rs' || name === 'mod.rs') return dirname(filePath);
  return join(dirname(filePath), basename(filePath, extname(filePath)));
}

function shouldSkipMod(name, skipForTest) {
  return skipForTest || name === 'tests' || name.endsWith('_tests');
}

function resolveChildModuleFile(parentFile, moduleName) {
  const root = moduleDirFor(parentFile);
  for (const c of [join(root, `${moduleName}.rs`), join(root, moduleName, 'mod.rs')]) {
    if (existsSync(c)) return c;
  }
  return null;
}

// --- Module discovery ---

export function discoverModules(crateRoot) {
  const modules = new Map(); // label -> filePath
  const pending = [{ path: [], file: resolve(crateRoot) }];

  while (pending.length) {
    const { path, file } = pending.pop();
    const label = path.join('::');
    if (modules.has(label)) continue;
    modules.set(label, file);

    let skipTest = false;
    for (const line of readFileSync(file, 'utf-8').split('\n')) {
      const s = line.trim();
      if (s.startsWith('#[cfg(') && s.includes('test')) { skipTest = true; continue; }
      if (s.startsWith('#[')) continue;

      const m = s.match(MOD_DECL_RE);
      if (!m) { skipTest = false; continue; }

      const name = m[1];
      if (shouldSkipMod(name, skipTest)) { skipTest = false; continue; }

      const child = resolveChildModuleFile(file, name);
      if (!child) {
        console.error(`warning: could not resolve module ${name} declared in ${file}`);
        skipTest = false;
        continue;
      }

      pending.push({ path: [...path, name], file: child });
      skipTest = false;
    }
  }
  return modules;
}

// --- Source processing ---

export function stripComments(source) {
  return source.replace(BLOCK_COMMENT_RE, '').replace(LINE_COMMENT_RE, '');
}

export function stripCfgTestItems(source) {
  const kept = [];
  let skipItem = false;
  let blockDepth = 0;

  for (const line of source.split('\n')) {
    const s = line.trim();

    if (blockDepth > 0) {
      blockDepth += countChar(s, '{');
      blockDepth -= countChar(s, '}');
      continue;
    }

    if (skipItem) {
      if (s.includes('{')) {
        blockDepth += countChar(s, '{');
        blockDepth -= countChar(s, '}');
      }
      if (s.endsWith(';')) skipItem = false;
      else if (blockDepth === 0) skipItem = false;
      continue;
    }

    if (s.startsWith('#[cfg(') && s.includes('test')) { skipItem = true; continue; }
    kept.push(line);
  }
  return kept.join('\n');
}

// --- Use-tree parsing ---

function splitTopLevelCommas(value) {
  const parts = [];
  let current = [];
  let depth = 0;
  for (const ch of value) {
    if (ch === '{') depth++;
    else if (ch === '}') depth--;
    else if (ch === ',' && depth === 0) {
      const piece = current.join('').trim();
      if (piece) parts.push(piece);
      current = [];
      continue;
    }
    current.push(ch);
  }
  const tail = current.join('').trim();
  if (tail) parts.push(tail);
  return parts;
}

export function expandUseTree(expr, prefix = '') {
  expr = expr.replace(/\s+/g, ' ').trim();
  if (!expr) return [];

  if (expr.startsWith('{') && expr.endsWith('}')) {
    return splitTopLevelCommas(expr.slice(1, -1)).flatMap(p => expandUseTree(p, prefix));
  }

  expr = expr.split(' as ')[0].trim();
  const brace = expr.indexOf('{');
  if (brace >= 0) {
    const base = expr.slice(0, brace).replace(/:+$/, '').trim();
    const inner = expr.slice(brace + 1, expr.lastIndexOf('}'));
    const next = prefix ? `${prefix}::${base}` : base;
    return splitTopLevelCommas(inner).flatMap(p => expandUseTree(p, next));
  }
  return [prefix ? `${prefix}::${expr}` : expr];
}

// --- Reference resolution ---

export function resolveReference(currentModule, rawRef, knownModules) {
  const cleaned = rawRef.trim().replace(/:+$/, '');
  if (!cleaned) return null;

  const parts = cleaned.split('::').filter(Boolean);
  if (!parts.length) return null;

  let absolute;
  if (parts[0] === 'crate') {
    absolute = parts.slice(1);
  } else if (parts[0] === 'self') {
    absolute = [...currentModule, ...parts.slice(1)];
  } else if (parts[0] === 'super') {
    const base = [...currentModule];
    let i = 0;
    while (i < parts.length && parts[i] === 'super') { if (base.length) base.pop(); i++; }
    absolute = [...base, ...parts.slice(i)];
  } else {
    return null;
  }
  if (!absolute.length) return null;

  for (let len = absolute.length; len > 0; len--) {
    const candidate = absolute.slice(0, len).join('::');
    if (knownModules.has(candidate)) return candidate;
  }
  return null;
}

// --- Dependency collection ---

export function collectDependencies(modules, { stripTestItems = true } = {}) {
  const known = new Set(modules.keys());
  const deps = new Map();

  for (const [label, filePath] of modules) {
    if (label === '') continue;
    let source = stripComments(readFileSync(filePath, 'utf-8'));
    if (stripTestItems) source = stripCfgTestItems(source);

    const modPath = label.split('::');
    const targets = new Set();

    for (const m of source.matchAll(USE_RE)) {
      for (const ref of expandUseTree(m[1])) {
        const t = resolveReference(modPath, ref, known);
        if (t) targets.add(t);
      }
    }
    for (const m of source.matchAll(PATH_TOKEN_RE)) {
      const t = resolveReference(modPath, m[0], known);
      if (t) targets.add(t);
    }
    if (targets.size) deps.set(label, targets);
  }
  return deps;
}

// --- Graph operations ---

export function collapseLabel(label, maxDepth) {
  const parts = label.split('::');
  return parts.length <= maxDepth ? label : parts.slice(0, maxDepth).join('::');
}

export function collapseGraph(modules, dependencies, maxDepth) {
  const nodes = new Set();
  for (const label of modules.keys()) {
    if (label !== '') nodes.add(collapseLabel(label, maxDepth));
  }

  const edges = new Map();
  for (const [source, targets] of dependencies) {
    const cs = collapseLabel(source, maxDepth);
    for (const target of targets) {
      const ct = collapseLabel(target, maxDepth);
      if (cs !== ct) {
        if (!edges.has(cs)) edges.set(cs, new Set());
        edges.get(cs).add(ct);
      }
    }
  }
  for (const node of nodes) if (!edges.has(node)) edges.set(node, new Set());
  return { nodes, edges };
}

export function tarjanScc(nodes, edges) {
  let idx = 0;
  const stack = [], onStack = new Set();
  const indices = new Map(), lowlinks = new Map();
  const components = [];

  function strongconnect(v) {
    indices.set(v, idx); lowlinks.set(v, idx); idx++;
    stack.push(v); onStack.add(v);

    for (const w of [...(edges.get(v) || [])].sort()) {
      if (!indices.has(w)) {
        strongconnect(w);
        lowlinks.set(v, Math.min(lowlinks.get(v), lowlinks.get(w)));
      } else if (onStack.has(w)) {
        lowlinks.set(v, Math.min(lowlinks.get(v), indices.get(w)));
      }
    }

    if (lowlinks.get(v) === indices.get(v)) {
      const component = [];
      while (true) {
        const w = stack.pop();
        onStack.delete(w);
        component.push(w);
        if (w === v) break;
      }
      components.push(component.sort());
    }
  }

  for (const v of [...nodes].sort()) if (!indices.has(v)) strongconnect(v);
  return components;
}

export function buildCondensationGraph(components, edges) {
  const compNodes = new Map();
  const nodeToComp = new Map();
  components.forEach((c, i) => { compNodes.set(i, c); c.forEach(n => nodeToComp.set(n, i)); });

  const compEdges = new Map();
  for (const [source, targets] of edges) {
    const sc = nodeToComp.get(source);
    if (sc === undefined) continue;
    for (const target of targets) {
      const tc = nodeToComp.get(target);
      if (tc === undefined || sc === tc) continue;
      if (!compEdges.has(sc)) compEdges.set(sc, new Set());
      compEdges.get(sc).add(tc);
    }
  }
  for (const id of compNodes.keys()) if (!compEdges.has(id)) compEdges.set(id, new Set());
  return { componentNodes: compNodes, condensationEdges: compEdges, nodeToComponent: nodeToComp };
}

export function topologicalOrder(componentNodes, condensationEdges) {
  const indegree = new Map();
  for (const id of componentNodes.keys()) indegree.set(id, 0);
  for (const targets of condensationEdges.values()) {
    for (const t of targets) indegree.set(t, (indegree.get(t) || 0) + 1);
  }

  const key = (id) => {
    const c = componentNodes.get(id);
    return [-c.length, c.join(',')];
  };
  const cmp = (a, b) => {
    const [as, al] = key(a), [bs, bl] = key(b);
    return as !== bs ? as - bs : (al < bl ? -1 : al > bl ? 1 : 0);
  };

  let ready = [...indegree].filter(([, d]) => d === 0).map(([id]) => id).sort(cmp);
  const ordered = [];

  while (ready.length) {
    const node = ready.shift();
    ordered.push(node);
    for (const t of [...(condensationEdges.get(node) || [])].sort(cmp)) {
      indegree.set(t, indegree.get(t) - 1);
      if (indegree.get(t) === 0) ready.push(t);
    }
    ready.sort(cmp);
  }

  if (ordered.length !== componentNodes.size) throw new Error('condensation graph contains a cycle');
  return ordered;
}

export function reverseEdges(edges) {
  const rev = new Map();
  for (const node of edges.keys()) if (!rev.has(node)) rev.set(node, new Set());
  for (const [source, targets] of edges) {
    for (const target of targets) {
      if (!rev.has(target)) rev.set(target, new Set());
      rev.get(target).add(source);
    }
  }
  return rev;
}

// --- Output ---

export function writeOutput(content, outputPath) {
  if (outputPath) {
    mkdirSync(dirname(resolve(outputPath)), { recursive: true });
    writeFileSync(resolve(outputPath), content, 'utf-8');
  } else {
    process.stdout.write(content);
  }
}
