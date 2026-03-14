# Deferred Architecture Friction

Items identified during architecture review that don't warrant action now
but have specific trigger conditions. Check this list when making changes
in the affected areas.

## Active Watch List

### `config.rs` re-exports engine layout vocabulary

`config.rs` re-exports `LayoutConfig`, `Ranker`, `LayoutDirection`,
`LabelDummyStrategy` from `engines::graph`. This couples the stable public
contract to engine-internal vocabulary.

**Trigger:** A second engine family (e.g., force-directed) introduces
conflicting config types that don't fit the current `LayoutConfig` shape.

**Action:** Generalize the config contract so engine-specific config lives
behind an engine-neutral envelope.

---

### Sequence `ParticipantKind` coupling

The Mermaid sequence AST re-exports `timeline::sequence::model::ParticipantKind`
instead of owning a parser-local enum. The compiler passes it through unchanged.
The layout phase stores it but never reads it.

**Trigger:** Sequence syntax expands with new participant kinds, or a second
frontend targets the timeline family.

**Action:** Introduce a parser-local participant kind and map it in the compiler.

---

### `simplification.rs` at the top level

A single-purpose path simplification module sitting alongside core contract
modules. Consumers are `config.rs`, `render/graph/svg/`, and `mmds/output.rs`.

**Trigger:** The module grows beyond path simplification, or the consumer list
expands enough to warrant a more specific home.

**Action:** Consider relocating under `graph/` or documenting it explicitly as
a shared geometry primitive.

---

### `internal_tests/` size

Seven cross-pipeline test suites totaling ~14.7k lines. These are legitimate
cross-boundary tests that need `pub(crate)` visibility, not an escape hatch.

**Trigger:** Suite count grows beyond 7, or individual suites grow large enough
that they're testing behavior that could be tested at owner boundaries with
small test-only builders.

**Action:** Continue rehoming tests to owner modules when feasible. Add
test-only fixture builders at owner boundaries to reduce cross-pipeline
test dependency.
