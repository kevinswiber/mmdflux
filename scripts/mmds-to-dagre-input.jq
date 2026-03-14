# Transform MMDS JSON output into dagre.js input format.
#
# Usage:
#   cargo run -- fixture.mmd --format mmds | jq -f scripts/mmds-to-dagre-input.jq
#
# The dagre input includes graph config (matching dagre/Mermaid defaults),
# nodes with dimensions, and edges with label dimensions.

# Map MMDS direction to dagre rankdir
def direction_to_rankdir:
  if . == "TD" then "TB"
  elif . == "BT" then "BT"
  elif . == "LR" then "LR"
  elif . == "RL" then "RL"
  else "TB"
  end;

# dagre layout defaults matching LayoutConfig::default()
def node_sep: 50;
def edge_sep: 20;
def base_rank_sep: 50;
def margin: 20;

# Add cluster offset when subgraphs are present
def rank_sep:
  if (.subgraphs | length) > 0 then base_rank_sep + 25
  else base_rank_sep
  end;

{
  graph: {
    rankdir: (.metadata.direction | direction_to_rankdir),
    nodesep: node_sep,
    edgesep: edge_sep,
    ranksep: rank_sep,
    marginx: margin,
    marginy: margin,
    ranker: "network-simplex"
  },
  nodes: (
    [.nodes[] | {
      id: .id,
      label: .label,
      width: .size.width,
      height: .size.height,
      parent: .parent,
      is_subgraph: false
    }] +
    [(.subgraphs // [])[] | {
      id: .id,
      label: .title,
      width: 0,
      height: 0,
      parent: .parent,
      is_subgraph: true
    }] | sort_by(.id)
  ),
  edges: [.edges | to_entries[] | {
    from: .value.source,
    to: .value.target,
    label: .value.label,
    label_width: (if .value.label then (.value.label | length) + 2 else 0 end),
    label_height: (if .value.label then 1 else 0 end),
    index: (.value.id | ltrimstr("e") | tonumber? // .key)
  }]
}
