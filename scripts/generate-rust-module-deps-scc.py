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
            "Generate a Mermaid condensation DAG for Rust module dependencies in the mmdflux crate."
        )
    )
    parser.add_argument(
        "--crate-root",
        default="src/lib.rs",
        help="Path to the crate root file to analyze (default: src/lib.rs).",
    )
    parser.add_argument(
        "--max-depth",
        type=int,
        default=1,
        help="Collapse module paths to this depth before computing SCCs (default: 1).",
    )
    parser.add_argument(
        "--direction",
        choices=["LR", "RL", "TB", "BT"],
        default="LR",
        help="Mermaid flowchart direction for the condensation DAG (default: LR).",
    )
    parser.add_argument(
        "--members-per-line",
        type=int,
        default=3,
        help="How many module names to place on each SCC label line (default: 3).",
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


def strip_cfg_test_items(source: str) -> str:
    kept: list[str] = []
    skip_next_item = False
    skip_block_depth = 0

    for line in source.splitlines():
        stripped = line.strip()

        if skip_block_depth > 0:
            skip_block_depth += stripped.count("{")
            skip_block_depth -= stripped.count("}")
            continue

        if skip_next_item:
            if "{" in stripped:
                skip_block_depth += stripped.count("{")
                skip_block_depth -= stripped.count("}")
            if stripped.endswith(";"):
                skip_next_item = False
            elif skip_block_depth == 0:
                skip_next_item = False
            continue

        if stripped.startswith("#[cfg(") and "test" in stripped:
            skip_next_item = True
            continue

        kept.append(line)

    return "\n".join(kept)


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

        source = strip_cfg_test_items(strip_comments(read_text(file_path)))

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


def scc_id(index: int) -> str:
    return f"scc_{index}"


def module_label(module_path: tuple[str, ...]) -> str:
    return "::".join(module_path)


def chunked(items: list[str], width: int) -> list[list[str]]:
    return [items[idx : idx + width] for idx in range(0, len(items), width)]


def scc_label(component: list[tuple[str, ...]], members_per_line: int) -> str:
    names = [module_label(member) for member in component]
    if len(component) == 1:
        return names[0]

    lines = [f"cycle ({len(component)} modules)"]
    lines.extend(", ".join(group) for group in chunked(names, members_per_line))
    return "<br/>".join(lines)


def build_condensation_graph(
    components: list[list[tuple[str, ...]]],
    edges: dict[tuple[str, ...], set[tuple[str, ...]]],
) -> tuple[dict[int, list[tuple[str, ...]]], dict[int, set[int]]]:
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

    return component_nodes, condensation_edges


def topological_order(
    component_nodes: dict[int, list[tuple[str, ...]]],
    condensation_edges: dict[int, set[int]],
) -> list[int]:
    indegree = {node: 0 for node in component_nodes}
    for targets in condensation_edges.values():
        for target in targets:
            indegree[target] += 1

    def sort_key(component_id: int) -> tuple[int, str]:
        component = component_nodes[component_id]
        largest_first = -len(component)
        label = ",".join(module_label(member) for member in component)
        return (largest_first, label)

    ready = deque(sorted((node for node, degree in indegree.items() if degree == 0), key=sort_key))
    ordered: list[int] = []

    while ready:
        node = ready.popleft()
        ordered.append(node)
        for target in sorted(condensation_edges[node], key=sort_key):
            indegree[target] -= 1
            if indegree[target] == 0:
                ready.append(target)
        ready = deque(sorted(ready, key=sort_key))

    if len(ordered) != len(component_nodes):
        raise RuntimeError("condensation graph unexpectedly contains a cycle")

    return ordered


def render_mermaid(
    component_nodes: dict[int, list[tuple[str, ...]]],
    condensation_edges: dict[int, set[int]],
    direction: str,
    crate_root_label: str,
    max_depth: int,
    members_per_line: int,
) -> str:
    ordered_components = topological_order(component_nodes, condensation_edges)
    multi_module_components = [
        component_id
        for component_id, members in component_nodes.items()
        if len(members) > 1
    ]
    single_module_components = [
        component_id
        for component_id, members in component_nodes.items()
        if len(members) == 1
    ]

    lines = [
        "%% Generated by scripts/generate-rust-module-deps-scc.py",
        f"%% crate-root: {crate_root_label}",
        f"%% max-depth: {max_depth}",
        f"%% component-count: {len(component_nodes)}",
        f"%% largest-scc-size: {max(len(members) for members in component_nodes.values())}",
        f"flowchart {direction}",
    ]

    for component_id in ordered_components:
        lines.append(
            f'    {scc_id(component_id)}["{scc_label(component_nodes[component_id], members_per_line)}"]'
        )

    for source in ordered_components:
        for target in sorted(condensation_edges[source]):
            lines.append(f"    {scc_id(source)} --> {scc_id(target)}")

    lines.extend(
        [
            "    classDef cycle fill:#fbe4e6,stroke:#b42318,stroke-width:2px,color:#1f2937;",
            "    classDef singleton fill:#e8f0ff,stroke:#4f46e5,stroke-width:1.5px,color:#1f2937;",
        ]
    )

    if multi_module_components:
        lines.append(
            "    class "
            + ",".join(scc_id(component_id) for component_id in sorted(multi_module_components))
            + " cycle;"
        )
    if single_module_components:
        lines.append(
            "    class "
            + ",".join(scc_id(component_id) for component_id in sorted(single_module_components))
            + " singleton;"
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

    crate_root_input = Path(args.crate_root)
    crate_root = crate_root_input.resolve()
    modules = discover_modules(crate_root)
    dependencies = collect_dependencies(modules)
    grouped_nodes, grouped_edges = collapse_graph(modules, dependencies, args.max_depth)
    components = tarjan_scc(grouped_nodes, grouped_edges)
    component_nodes, condensation_edges = build_condensation_graph(components, grouped_edges)
    diagram = render_mermaid(
        component_nodes,
        condensation_edges,
        direction=args.direction,
        crate_root_label=crate_root_input.as_posix(),
        max_depth=args.max_depth,
        members_per_line=args.members_per_line,
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
