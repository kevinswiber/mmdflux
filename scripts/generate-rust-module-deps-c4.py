#!/usr/bin/env python3

from __future__ import annotations

import argparse
import re
import sys
from collections import defaultdict
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
        description="Generate a Mermaid C4 module dependency map for the Rust mmdflux crate."
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
        help="Collapse module paths to this depth when emitting components (default: 1).",
    )
    parser.add_argument(
        "--shapes-per-row",
        type=int,
        default=4,
        help="C4 layout hint for shapes per row (default: 4).",
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
        expanded = []
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


def component_id(module_path: tuple[str, ...]) -> str:
    return "mod_" + "_".join(module_path)


def component_label(module_path: tuple[str, ...]) -> str:
    return "::".join(module_path)


def component_kind(
    module_path: tuple[str, ...],
    grouped_members: dict[tuple[str, ...], set[tuple[str, ...]]],
) -> str:
    if len(grouped_members[module_path]) > 1:
        return "Rust module group"
    return "Rust module"


def crate_name(crate_root: Path) -> str:
    return crate_root.parent.parent.name or "crate"


def render_mermaid(
    modules: dict[tuple[str, ...], Path],
    dependencies: dict[tuple[str, ...], set[tuple[str, ...]]],
    max_depth: int,
    shapes_per_row: int,
    crate_root: Path,
    crate_root_label: str,
) -> str:
    grouped_nodes = {
        collapse_module_path(module_path, max_depth)
        for module_path in modules
        if module_path
    }

    grouped_members: dict[tuple[str, ...], set[tuple[str, ...]]] = defaultdict(set)
    for module_path in modules:
        if not module_path:
            continue
        grouped_members[collapse_module_path(module_path, max_depth)].add(module_path)

    grouped_edges: set[tuple[tuple[str, ...], tuple[str, ...]]] = set()
    for source, targets in dependencies.items():
        collapsed_source = collapse_module_path(source, max_depth)
        for target in targets:
            collapsed_target = collapse_module_path(target, max_depth)
            if collapsed_source == collapsed_target:
                continue
            grouped_edges.add((collapsed_source, collapsed_target))

    lines = [
        "%% Generated by scripts/generate-rust-module-deps-c4.py",
        f"%% crate-root: {crate_root_label}",
        f"%% max-depth: {max_depth}",
        "C4Component",
        f'UpdateLayoutConfig($c4ShapeInRow="{shapes_per_row}", $c4BoundaryInRow="1")',
        f'Boundary(main_crate, "{crate_name(crate_root)} crate", "Rust crate") {{',
    ]

    for node in sorted(grouped_nodes):
        lines.append(
            f'    Component({component_id(node)}, "{component_label(node)}", "{component_kind(node, grouped_members)}")'
        )

    lines.append("}")

    for source, target in sorted(grouped_edges):
        lines.append(
            f'    Rel({component_id(source)}, {component_id(target)}, "uses")'
        )

    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    if args.max_depth < 1:
        print("--max-depth must be >= 1", file=sys.stderr)
        return 2
    if args.shapes_per_row < 1:
        print("--shapes-per-row must be >= 1", file=sys.stderr)
        return 2

    crate_root_input = Path(args.crate_root)
    crate_root = crate_root_input.resolve()
    modules = discover_modules(crate_root)
    dependencies = collect_dependencies(modules)
    diagram = render_mermaid(
        modules,
        dependencies,
        max_depth=args.max_depth,
        shapes_per_row=args.shapes_per_row,
        crate_root=crate_root,
        crate_root_label=crate_root_input.as_posix(),
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
