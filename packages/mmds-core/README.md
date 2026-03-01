# @mmds/core

Shared MMDS types and normalization helpers used by adapter packages.

## Features

- MMDS defaults expansion for nodes and edges.
- Subgraph parent-chain and descendant traversal helpers.
- Endpoint intent helpers for `from_subgraph` / `to_subgraph` edge intent.

## Usage

```ts
import { normalizeMmds } from "@mmds/core";

const normalized = normalizeMmds(doc);
```
