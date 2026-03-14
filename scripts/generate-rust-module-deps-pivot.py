#!/usr/bin/env python3

from __future__ import annotations

import argparse
import re
import sys
from collections import defaultdict, deque
from pathlib import Path


MOD_DECL_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;\s*$"
)
USE_RE = re.compile(r"\b(?:pub\s+)?use\s+([^;]+);", re.MULTILINE | re.DOTALL)
PATH_TOKEN_RE = re.compile(r"\b(?:crate|self|super)(?:::[A-Za-z_][A-Za-z0-9_]*)+")
BLOCK_COMMENT_RE = re.compile(r"/\*.*?\*/", re.DOTALL)
LINE_COMMENT_RE = re.compile(r"//.*?$", re.MULTILINE)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Generate a pivoted Mermaid dependency view for Rust modules in the mmdflux crate."
        )
    )
    parser.add_argument(
        "--crate-root",
        default="src/lib.rs",
        help="Path to the crate root file to analyze (default: src/lib.rs).",
    )
    parser.add_argument(
        "--module",
        required=True,
        help=(
            "Module label to pivot on, such as 'runtime' at max-depth 1 or "
            "'runtime::facade' at max-depth 2."
        ),
    )
    parser.add_argument(
        "--max-depth",
        type=int,
        default=1,
        help="Collapse module paths to this depth before rendering (default: 1).",
    )
    parser.add_argument(
        "--direction",
        choices=["outbound", "inbound", "both"],
        default="outbound",
        help="Traversal direction from the pivot module (default: outbound).",
    )
    parser.add_argument(
        "--mode",
        choices=["tree", "dag"],
        default="tree",
        help="Render a spanning tree or the reachable DAG/subgraph (default: tree).",
    )
    parser.add_argument(
        "--condense-scc",
        action="store_true",
        help="Operate on the SCC-condensed graph instead of raw modules.",
    )
    parser.add_argument(
        "--direction-layout",
        choices=["LR", "RL", "TB", "BT"],
        default="LR",
        help="Mermaid flowchart direction (default: LR).",
    )
    parser.add_argument(
        "--members-per-line",
        type=int,
        default=3,
        help="How many module names to place on each SCC label line when condensed (default: 3).",
    )
    parser.add_argument(
        "--output",
        help="Write the generated Mermaid diagram to this file instead of stdout.",
    )
    return parser.parse_args()


def module_dir_for(file_path: Path) -> Path:
    if file_path.name in {"lib.rs", "main.rs", "mod.rs"}:
        return file_path.parent
    return file_path.parent / file_path.stem


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def should_skip_mod(name: str, skip_for_test: bool) -> bool:
    return skip_for_test or name == "tests" or name.endswith("_tests")


def resolve_child_module_file(parent_file: Path, module_name: str) -> Path | None:
    search_root = module_dir_for(parent_file)
    candidates = [search_root / f"{module_name}.rs", search_root / module_name / "mod.rs"]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return None


def discover_modules(crate_root: Path) -> dict[tuple[str, ...], Path]:
    modules: dict[tuple[str, ...], Path] = {}
    pending: list[tuple[tuple[str, ...], Path]] = [((), crate_root)]

    while pending:
        module_path, file_path = pending.pop()
        if module_path in modules:
            continue
        modules[module_path] = file_path

        skip_next_mod_for_test = False
        for line in read_text(file_path).splitlines():
            stripped = line.strip()
            if stripped.startswith("#[cfg(") and "test" in stripped:
                skip_next_mod_for_test = True
                continue
            if stripped.startswith("#["):
                continue

            match = MOD_DECL_RE.match(stripped)
            if not match:
                skip_next_mod_for_test = False
                continue

            module_name = match.group(1)
            if should_skip_mod(module_name, skip_next_mod_for_test):
                skip_next_mod_for_test = False
                continue

            child_file = resolve_child_module_file(file_path, module_name)
            if child_file is None:
                print(
                    f"warning: could not resolve module {module_name} declared in {file_path}",
                    file=sys.stderr,
                )
                skip_next_mod_for_test = False
                continue

            pending.append((module_path + (module_name,), child_file))
            skip_next_mod_for_test = False

    return modules


def strip_comments(source: str) -> str:
    without_block_comments = BLOCK_COMMENT_RE.sub("", source)
    return LINE_COMMENT_RE.sub("", without_block_comments)


def split_top_level_commas(value: str) -> list[str]:
    parts: list[str] = []
    current: list[str] = []
    depth = 0

    for ch in value:
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        elif ch == "," and depth == 0:
            piece = "".join(current).strip()
            if piece:
                parts.append(piece)
            current = []
            continue
        current.append(ch)

    tail = "".join(current).strip()
    if tail:
        parts.append(tail)
    return parts


def expand_use_tree(expr: str, prefix: str = "") -> list[str]:
    expr = " ".join(expr.split())
    expr = expr.strip()
    if not expr:
        return []

    if expr.startswith("{") and expr.endswith("}"):
        inner = expr[1:-1]
        expanded: list[str] = []
        for part in split_top_level_commas(inner):
            expanded.extend(expand_use_tree(part, prefix))
        return expanded

    expr = expr.split(" as ", 1)[0].strip()
    brace_index = expr.find("{")
    if brace_index >= 0:
        base = expr[:brace_index].rstrip(":").strip()
        inner = expr[brace_index + 1 : expr.rfind("}")]
        next_prefix = f"{prefix}::{base}" if prefix else base
        expanded: list[str] = []
        for part in split_top_level_commas(inner):
            expanded.extend(expand_use_tree(part, next_prefix))
        return expanded

    return [f"{prefix}::{expr}" if prefix else expr]


def resolve_reference(
    current_module: tuple[str, ...], raw_ref: str, known_modules: set[tuple[str, ...]]
) -> tuple[str, ...] | None:
    cleaned = raw_ref.strip().rstrip(":")
    if not cleaned:
        return None

    parts = [part for part in cleaned.split("::") if part]
    if not parts:
        return None

    if parts[0] == "crate":
        absolute = parts[1:]
    elif parts[0] == "self":
        absolute = list(current_module) + parts[1:]
    elif parts[0] == "super":
        base = list(current_module)
        index = 0
        while index < len(parts) and parts[index] == "super":
            if base:
                base.pop()
            index += 1
        absolute = base + parts[index:]
    else:
        return None

    if not absolute:
        return None

    for prefix_len in range(len(absolute), 0, -1):
        candidate = tuple(absolute[:prefix_len])
        if candidate in known_modules:
            return candidate
    return None


def collect_dependencies(
    modules: dict[tuple[str, ...], Path],
) -> dict[tuple[str, ...], set[tuple[str, ...]]]:
    known_modules = set(modules)
    dependencies: dict[tuple[str, ...], set[tuple[str, ...]]] = defaultdict(set)

    for module_path, file_path in modules.items():
        if not module_path:
            continue

        source = strip_comments(read_text(file_path))

        for use_body in USE_RE.findall(source):
            for raw_ref in expand_use_tree(use_body):
                target = resolve_reference(module_path, raw_ref, known_modules)
                if target is not None:
                    dependencies[module_path].add(target)

        for raw_ref in PATH_TOKEN_RE.findall(source):
            target = resolve_reference(module_path, raw_ref, known_modules)
            if target is not None:
                dependencies[module_path].add(target)

    return dependencies


def collapse_module_path(module_path: tuple[str, ...], max_depth: int) -> tuple[str, ...]:
    if len(module_path) <= max_depth:
        return module_path
    return module_path[:max_depth]


def collapse_graph(
    modules: dict[tuple[str, ...], Path],
    dependencies: dict[tuple[str, ...], set[tuple[str, ...]]],
    max_depth: int,
) -> tuple[set[tuple[str, ...]], dict[tuple[str, ...], set[tuple[str, ...]]]]:
    grouped_nodes = {
        collapse_module_path(module_path, max_depth)
        for module_path in modules
        if module_path
    }

    grouped_edges: dict[tuple[str, ...], set[tuple[str, ...]]] = defaultdict(set)
    for source, targets in dependencies.items():
        collapsed_source = collapse_module_path(source, max_depth)
        for target in targets:
            collapsed_target = collapse_module_path(target, max_depth)
            if collapsed_source == collapsed_target:
                continue
            grouped_edges[collapsed_source].add(collapsed_target)

    for node in grouped_nodes:
        grouped_edges.setdefault(node, set())

    return grouped_nodes, grouped_edges


def tarjan_scc(
    nodes: set[tuple[str, ...]],
    edges: dict[tuple[str, ...], set[tuple[str, ...]]],
) -> list[list[tuple[str, ...]]]:
    index = 0
    stack: list[tuple[str, ...]] = []
    on_stack: set[tuple[str, ...]] = set()
    indices: dict[tuple[str, ...], int] = {}
    lowlinks: dict[tuple[str, ...], int] = {}
    components: list[list[tuple[str, ...]]] = []

    def strongconnect(node: tuple[str, ...]) -> None:
        nonlocal index
        indices[node] = index
        lowlinks[node] = index
        index += 1
        stack.append(node)
        on_stack.add(node)

        for neighbor in sorted(edges.get(node, ())):
            if neighbor not in indices:
                strongconnect(neighbor)
                lowlinks[node] = min(lowlinks[node], lowlinks[neighbor])
            elif neighbor in on_stack:
                lowlinks[node] = min(lowlinks[node], indices[neighbor])

        if lowlinks[node] == indices[node]:
            component: list[tuple[str, ...]] = []
            while True:
                member = stack.pop()
                on_stack.remove(member)
                component.append(member)
                if member == node:
                    break
            components.append(sorted(component))

    for node in sorted(nodes):
        if node not in indices:
            strongconnect(node)

    return components


def build_condensation_graph(
    components: list[list[tuple[str, ...]]],
    edges: dict[tuple[str, ...], set[tuple[str, ...]]],
) -> tuple[dict[int, list[tuple[str, ...]]], dict[int, set[int]], dict[tuple[str, ...], int]]:
    component_nodes = {index: component for index, component in enumerate(components)}
    node_to_component: dict[tuple[str, ...], int] = {}
    for index, component in component_nodes.items():
        for node in component:
            node_to_component[node] = index

    condensation_edges: dict[int, set[int]] = defaultdict(set)
    for source, targets in edges.items():
        source_component = node_to_component[source]
        for target in targets:
            target_component = node_to_component[target]
            if source_component != target_component:
                condensation_edges[source_component].add(target_component)

    for component_id in component_nodes:
        condensation_edges.setdefault(component_id, set())

    return component_nodes, condensation_edges, node_to_component


def module_label(module_path: tuple[str, ...]) -> str:
    return "::".join(module_path)


def node_id(label: str) -> str:
    safe = re.sub(r"[^A-Za-z0-9_]+", "_", label).strip("_")
    return f"node_{safe or 'root'}"


def reverse_edges(edges: dict[str, set[str]]) -> dict[str, set[str]]:
    reversed_edges: dict[str, set[str]] = {node: set() for node in edges}
    for source, targets in edges.items():
        for target in targets:
            reversed_edges.setdefault(target, set()).add(source)
        reversed_edges.setdefault(source, set())
    return reversed_edges


def chunked(items: list[str], width: int) -> list[list[str]]:
    return [items[idx : idx + width] for idx in range(0, len(items), width)]


def scc_label(component: list[tuple[str, ...]], members_per_line: int) -> str:
    names = [module_label(member) for member in component]
    if len(component) == 1:
        return names[0]

    lines = [f"cycle ({len(component)} modules)"]
    lines.extend(", ".join(group) for group in chunked(names, members_per_line))
    return "<br/>".join(lines)


def build_render_graph(
    nodes: set[tuple[str, ...]],
    edges: dict[tuple[str, ...], set[tuple[str, ...]]],
    condense_scc: bool,
    members_per_line: int,
) -> tuple[
    dict[str, str],
    dict[str, str],
    dict[str, set[str]],
    dict[str, str],
    dict[str, str],
]:
    if not condense_scc:
        raw_to_render = {module_label(node): module_label(node) for node in nodes}
        display_labels = dict(raw_to_render)
        render_edges = {
            module_label(source): {module_label(target) for target in targets}
            for source, targets in edges.items()
        }
        render_edges = {label: render_edges.get(label, set()) for label in display_labels}
        node_styles = {label: "normal" for label in display_labels}
        node_ids = {label: node_id(label) for label in display_labels}
        return raw_to_render, display_labels, render_edges, node_styles, node_ids

    components = tarjan_scc(nodes, edges)
    component_nodes, condensation_edges, node_to_component = build_condensation_graph(
        components, edges
    )

    display_labels = {
        f"scc::{component_id}": scc_label(component_nodes[component_id], members_per_line)
        for component_id in component_nodes
    }
    render_edges = {
        f"scc::{component_id}": {f"scc::{target}" for target in targets}
        for component_id, targets in condensation_edges.items()
    }
    node_styles = {
        f"scc::{component_id}": len(component_nodes[component_id]) > 1
        for component_id in component_nodes
    }
    node_ids = {label: node_id(label) for label in display_labels}
    raw_to_render = {
        module_label(node): f"scc::{node_to_component[node]}"
        for node in nodes
    }
    style_labels = {
        label: ("cycle" if is_cycle else "normal")
        for label, is_cycle in node_styles.items()
    }
    return raw_to_render, display_labels, render_edges, style_labels, node_ids


def resolve_pivot_label(
    requested_module: str,
    raw_nodes: set[tuple[str, ...]],
    raw_label_to_render_label: dict[str, str],
) -> str:
    normalized = requested_module.strip()
    available = sorted(module_label(node) for node in raw_nodes)
    if normalized not in raw_label_to_render_label:
        choices = ", ".join(available)
        raise ValueError(
            f"unknown module '{requested_module}' for this max-depth. Available modules: {choices}"
        )
    return raw_label_to_render_label[normalized]


def tree_edges_from_pivot(
    pivot: str,
    edges: dict[str, set[str]],
    direction: str,
) -> tuple[set[str], set[tuple[str, str]]]:
    if direction == "both":
        raise ValueError("tree mode requires --direction inbound or outbound")

    traversal_edges = edges if direction == "outbound" else reverse_edges(edges)
    visited = {pivot}
    tree_edges: set[tuple[str, str]] = set()
    queue: deque[str] = deque([pivot])

    while queue:
        current = queue.popleft()
        for neighbor in sorted(traversal_edges.get(current, ())):
            if neighbor in visited:
                continue
            visited.add(neighbor)
            queue.append(neighbor)
            if direction == "outbound":
                tree_edges.add((current, neighbor))
            else:
                tree_edges.add((neighbor, current))

    return visited, tree_edges


def reachable_nodes(
    pivot: str,
    edges: dict[str, set[str]],
    direction: str,
) -> set[str]:
    forward = edges
    backward = reverse_edges(edges)

    def walk(start: str, graph: dict[str, set[str]]) -> set[str]:
        seen = {start}
        queue: deque[str] = deque([start])
        while queue:
            current = queue.popleft()
            for neighbor in sorted(graph.get(current, ())):
                if neighbor in seen:
                    continue
                seen.add(neighbor)
                queue.append(neighbor)
        return seen

    if direction == "outbound":
        return walk(pivot, forward)
    if direction == "inbound":
        return walk(pivot, backward)
    return walk(pivot, forward) | walk(pivot, backward)


def dag_subgraph_edges(
    included_nodes: set[str],
    edges: dict[str, set[str]],
) -> set[tuple[str, str]]:
    return {
        (source, target)
        for source, targets in edges.items()
        if source in included_nodes
        for target in targets
        if target in included_nodes
    }


def render_mermaid(
    labels: dict[str, str],
    node_ids: dict[str, str],
    node_styles: dict[str, str],
    included_nodes: set[str],
    edge_pairs: set[tuple[str, str]],
    requested_module: str,
    pivot_label: str,
    crate_root_label: str,
    max_depth: int,
    direction_layout: str,
    mode: str,
    direction: str,
    condensed: bool,
) -> str:
    ordered_nodes = sorted(included_nodes)
    lines = [
        "%% Generated by scripts/generate-rust-module-deps-pivot.py",
        f"%% crate-root: {crate_root_label}",
        f"%% max-depth: {max_depth}",
        f"%% pivot-request: {requested_module}",
        f"%% pivot-node: {pivot_label}",
        f"%% mode: {mode}",
        f"%% direction: {direction}",
        f"%% condensed-scc: {'yes' if condensed else 'no'}",
        f"flowchart {direction_layout}",
    ]

    for node in ordered_nodes:
        lines.append(f'    {node_ids[node]}["{labels[node]}"]')

    for source, target in sorted(edge_pairs):
        lines.append(f"    {node_ids[source]} --> {node_ids[target]}")

    lines.extend(
        [
            "    classDef pivot fill:#fde68a,stroke:#b45309,stroke-width:2px,color:#1f2937;",
            "    classDef cycle fill:#fbe4e6,stroke:#b42318,stroke-width:2px,color:#1f2937;",
            "    classDef normal fill:#e8f0ff,stroke:#4f46e5,stroke-width:1.5px,color:#1f2937;",
        ]
    )

    lines.append(f"    class {node_ids[pivot_label]} pivot;")

    cycle_nodes = [
        node
        for node, style in node_styles.items()
        if style == "cycle" and node in included_nodes and node != pivot_label
    ]
    normal_nodes = [
        node
        for node, style in node_styles.items()
        if style == "normal" and node in included_nodes and node != pivot_label
    ]

    if cycle_nodes:
        lines.append(
            "    class " + ",".join(node_ids[node] for node in sorted(cycle_nodes)) + " cycle;"
        )
    if normal_nodes:
        lines.append(
            "    class " + ",".join(node_ids[node] for node in sorted(normal_nodes)) + " normal;"
        )

    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    if args.max_depth < 1:
        print("--max-depth must be >= 1", file=sys.stderr)
        return 2
    if args.members_per_line < 1:
        print("--members-per-line must be >= 1", file=sys.stderr)
        return 2
    if args.mode == "tree" and args.direction == "both":
        print("--mode tree requires --direction inbound or outbound", file=sys.stderr)
        return 2

    crate_root_input = Path(args.crate_root)
    crate_root = crate_root_input.resolve()
    modules = discover_modules(crate_root)
    dependencies = collect_dependencies(modules)
    raw_nodes, raw_edges = collapse_graph(modules, dependencies, args.max_depth)
    raw_label_to_render_label, display_labels, render_edges, node_styles, node_ids = build_render_graph(
        raw_nodes,
        raw_edges,
        condense_scc=args.condense_scc,
        members_per_line=args.members_per_line,
    )

    try:
        pivot_label = resolve_pivot_label(args.module, raw_nodes, raw_label_to_render_label)
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 2

    if args.mode == "tree":
        included_nodes, edge_pairs = tree_edges_from_pivot(
            pivot_label,
            render_edges,
            direction=args.direction,
        )
    else:
        included_nodes = reachable_nodes(pivot_label, render_edges, direction=args.direction)
        edge_pairs = dag_subgraph_edges(included_nodes, render_edges)

    diagram = render_mermaid(
        labels=display_labels,
        node_ids=node_ids,
        node_styles=node_styles,
        included_nodes=included_nodes,
        edge_pairs=edge_pairs,
        requested_module=args.module,
        pivot_label=pivot_label,
        crate_root_label=crate_root_input.as_posix(),
        max_depth=args.max_depth,
        direction_layout=args.direction_layout,
        mode=args.mode,
        direction=args.direction,
        condensed=args.condense_scc,
    )

    if args.output:
        output_path = Path(args.output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(diagram, encoding="utf-8")
    else:
        sys.stdout.write(diagram)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
