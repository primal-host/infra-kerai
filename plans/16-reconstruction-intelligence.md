# Plan 16: Reconstruction Intelligence

*Depends on: Plan 02 (Rust Parser), Plan 03 (Source Reconstruction), Plan 15 (Comment Handling)*
*Status: Planned*

## Goal

Make the reconstruction pipeline smarter than a formatter. Three features, each independently togglable:

1. **Import sorting** — Canonical `use` ordering (std → external → crate) during reconstruction
2. **Derive ordering** — Alphabetical `#[derive(...)]` normalization during reconstruction
3. **Inline suggestions** — Advisory `// kerai:` comments for improvements kerai can detect but shouldn't auto-apply, with dismissal tracking across parse cycles

The key insight: kerai's parse→store→reconstruct cycle creates a natural feedback loop. Suggestions are emitted once. If the developer removes the comment without changing the code, kerai marks it dismissed and never re-emits it. If the developer applies the suggestion and the code changes, kerai marks it applied. This makes kerai a persistent code advisor with memory — not a nagging linter.

## Design Principles

- **Non-destructive by default.** Import sorting and derive ordering change whitespace/ordering only. Suggestions are comments, not code changes.
- **Opt-out via source comments.** `// kerai:skip-sort-imports` at file top disables import sorting for that file. Mirrors `// rustfmt::skip` convention.
- **One-shot suggestions.** Each suggestion is emitted exactly once per target. Dismissed suggestions are never re-emitted. Applied suggestions are tracked for metrics.
- **Uses existing data model.** Suggestions are nodes with `suggests` edges. No new tables needed.

## Control Flags

Flags follow Rust's attribute convention — inline comments that kerai recognizes during parsing:

| Flag | Scope | Effect |
|------|-------|--------|
| `// kerai:skip-sort-imports` | File | Don't reorder `use` statements in this file |
| `// kerai:skip-order-derives` | File | Don't reorder `derive` attributes in this file |
| `// kerai:skip-suggestions` | File | Don't emit any `// kerai:` suggestion comments |
| `// kerai:skip` | File | Disable all reconstruction intelligence for this file |

Flags are stored as metadata on the file node during parsing (`metadata.kerai_flags`). The reconstruction pipeline checks flags before applying transformations.

Additionally, the `reconstruct_file` and `reconstruct_crate` functions gain an optional `options` parameter:

```sql
-- Default: all features enabled
SELECT kerai.reconstruct_file(file_id);

-- Disable suggestions
SELECT kerai.reconstruct_file(file_id, '{"suggestions": false}'::jsonb);

-- Raw reconstruction (no sorting, no suggestions, just formatting)
SELECT kerai.reconstruct_file(file_id, '{"sort_imports": false, "order_derives": false, "suggestions": false}'::jsonb);
```

Function-level options override file-level flags (so you can get a raw reconstruction even if the file doesn't have skip flags).

## Deliverables

### 16.1 Import Sorting

Sort `use` statements in the assembler before emitting them. Groups:

```rust
// Group 1: std, core, alloc
use std::collections::HashMap;
use std::io;

// Group 2: external crates (alphabetical)
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Group 3: crate-internal (crate::, self::, super::)
use crate::parser::kinds::Kind;
use super::formatter;
```

**Rules:**
- Within each group, sort alphabetically by the full path
- Blank line between groups
- Merge detection: if two `use` nodes have identical paths, emit once (dedup)
- Nested imports (`use std::{io, fs}`) — sort the nested items alphabetically
- `use` nodes that are inside `impl` or `fn` blocks are NOT sorted (only top-level file imports)

**Implementation:** In `assembler.rs`, before emitting children, partition `use`-kind nodes from other items. Sort the use nodes into groups, emit them first (with group separators), then emit remaining items in position order.

The assembler already controls emission order — this is a reordering of existing nodes, not a code transformation.

### 16.2 Derive Ordering

Normalize `#[derive(...)]` attributes to alphabetical order during reconstruction.

```rust
// Before
#[derive(Serialize, Clone, Debug, Eq, PartialEq, Hash)]

// After
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
```

**Implementation:** This operates on the `source` metadata of items. During reconstruction, when an item's source contains `#[derive(...)`, parse the trait list, sort alphabetically, and rewrite the attribute in the emitted source.

A simple regex + split approach is sufficient — derive attributes have predictable syntax. No need for a full attribute parser.

Edge case: multiple `#[derive(...)]` on the same item — sort within each, don't merge across them (merging changes semantics in some proc-macro cases).

### 16.3 Suggestion Node Kind and Edge Relation

Add to `kinds.rs`:

```rust
Suggestion,  // "suggestion" — advisory comment node
```

Edge relation: `"suggests"` — from suggestion node to the target node it's about.

Suggestion node structure:

```rust
NodeRow {
    kind: Kind::Suggestion,
    content: "consider &str instead of &String",  // human-readable suggestion text
    parent_id: file_node_id,                       // parented to the file
    position: target_line,                         // line number of the target
    metadata: {
        "rule": "prefer_str_slice",           // machine-readable rule ID
        "status": "emitted",                  // emitted | dismissed | applied
        "target_hash": "a1b2c3d4",           // hash of the target node's content at emission time
        "severity": "info",                   // info | warning
        "category": "idiom",                  // idiom | performance | naming | dead_code
    }
}
```

### 16.4 Suggestion Rules

Initial set of high-confidence rules. Each rule has an ID, a detection function, and a message template.

#### Idiom Rules

| Rule ID | Detects | Message |
|---------|---------|---------|
| `prefer_str_slice` | `fn foo(s: &String)` | `consider &str instead of &String` |
| `prefer_slice` | `fn foo(v: &Vec<T>)` | `consider &[T] instead of &Vec<T>` |
| `prefer_as_ref` | `fn foo(s: &Option<String>)` | `consider Option<&str> or .as_ref()` |
| `clone_on_copy` | `.clone()` on a `Copy` type | `clone() is unnecessary on Copy types` |
| `string_to_string` | `s.to_string()` where `s: String` | `already a String, to_string() is redundant` |

#### Naming Rules

| Rule ID | Detects | Message |
|---------|---------|---------|
| `non_snake_fn` | `fn myFunc()` | `function names should be snake_case: my_func` |
| `non_snake_var` | `let myVar = ...` | `variable names should be snake_case: my_var` |
| `non_camel_type` | `struct my_struct` | `type names should be CamelCase: MyStruct` |
| `non_upper_const` | `const my_const: ...` | `constants should be UPPER_SNAKE_CASE: MY_CONST` |

#### Dead Code Rules

| Rule ID | Detects | Message |
|---------|---------|---------|
| `unused_import` | `use` node with no references from other nodes in the crate | `this import appears unused` |
| `unused_pub` | `pub fn`/`pub struct` with no cross-file references | `this pub item has no external references in the crate` |

Dead code rules are unique to kerai — they require cross-file reference analysis that line-based linters can't do. The edges table already tracks `references` and `calls` relations, making these queries straightforward.

#### Missing Attribute Rules

| Rule ID | Detects | Message |
|---------|---------|---------|
| `missing_must_use` | `fn` that returns a value and has no side effects (no `&mut self`, no SPI calls) | `consider #[must_use] on this pure function` |
| `missing_derive_debug` | `struct`/`enum` without `Debug` derive | `consider deriving Debug` |

### 16.5 Suggestion Detection Pipeline

Runs during `parse_single_file()`, after AST walking but before insertion:

```
AST walk (nodes + edges)
  → run suggestion rules against nodes
    → for each finding, check if a dismissed suggestion already exists for this target + rule
      → if dismissed: skip
      → if new: create suggestion node + suggests edge
```

**Dismissal check:** Query existing suggestion nodes for the same file where `rule` matches and `status = 'dismissed'`. The `target_hash` in metadata lets us distinguish "dismissed for this exact code" vs. "code changed, re-evaluate."

If the target node's content hash has changed since dismissal, the dismissal is void and the suggestion can be re-emitted (the developer changed the code but the issue reappeared).

### 16.6 Suggestion Status Tracking

During parsing, when kerai encounters a `// kerai:` comment:

1. **Find the matching suggestion node** by rule ID + target node
2. **Check if the comment is still present** in the re-parsed source
3. **Compare the target code** to the stored `target_hash`

State transitions:

```
[emitted] --comment removed, code unchanged--> [dismissed]
[emitted] --comment removed, code changed----> [applied]
[emitted] --comment still present------------> [emitted] (no change)
[dismissed] --code changed, issue recurs------> [emitted] (re-emit)
[dismissed] --code unchanged------------------> [dismissed] (stays quiet)
[applied]  ---------------------------------> (archived, no further action)
```

### 16.7 Reconstruction Emission

In `assembler.rs`, after assembling an item's source:

1. Query suggestion nodes for this file with `status = 'emitted'`
2. Group suggestions by their target node
3. Before emitting each item, check if it has pending suggestions
4. Emit `// kerai: [message]` on the line above the item

```rust
// kerai: consider &str instead of &String (prefer_str_slice)
fn process(input: &String) -> bool {
    // ...
}
```

The `(rule_id)` suffix lets kerai match the comment back to its suggestion node during the next parse cycle.

### 16.8 Flag Parsing

During comment extraction, detect `// kerai:` prefixed comments:

- `// kerai:skip-*` flags → store in file node metadata, don't create comment nodes
- `// kerai: [message] (rule_id)` suggestion comments → match to suggestion nodes for status tracking

This reuses the existing comment extraction pipeline (Plan 15) with an additional classification step.

## Files to Modify

| File | Change |
|------|--------|
| `src/parser/kinds.rs` | Add `Suggestion` variant |
| `src/parser/mod.rs` | Add suggestion detection pipeline, flag parsing |
| `src/parser/ast_walker.rs` | Expose type info for idiom rule detection |
| `src/reconstruct/assembler.rs` | Import sorting, derive ordering, suggestion emission |
| `src/reconstruct/mod.rs` | Add `options` parameter to reconstruct functions |
| `src/reconstruct/import_sorter.rs` | **New.** Import grouping and sorting logic |
| `src/reconstruct/derive_orderer.rs` | **New.** Derive attribute normalization |
| `src/reconstruct/suggestions.rs` | **New.** Suggestion rule engine and emission |
| `src/parser/suggestion_rules.rs` | **New.** Rule definitions and detection functions |
| `src/parser/flag_parser.rs` | **New.** `// kerai:` comment classifier |
| `src/schema.rs` | No changes (suggestions use existing nodes/edges tables) |
| `src/lib.rs` | New tests |

## Implementation Order

```
Phase 1: Import Sorting (16.1)
  ├── import_sorter.rs (sorting logic, unit tests)
  └── assembler.rs (integration)

Phase 2: Derive Ordering (16.2)
  ├── derive_orderer.rs (ordering logic, unit tests)
  └── assembler.rs (integration)

Phase 3: Suggestion Infrastructure (16.3, 16.5, 16.6, 16.8)
  ├── kinds.rs (Suggestion variant)
  ├── flag_parser.rs (kerai comment classifier)
  ├── suggestion_rules.rs (rule definitions)
  └── parser/mod.rs (detection + status tracking)

Phase 4: Suggestion Emission (16.4, 16.7)
  ├── suggestions.rs (emission logic)
  └── assembler.rs (integration)

Phase 5: Control Flags
  ├── flag_parser.rs (skip flags)
  ├── assembler.rs (flag checking)
  └── reconstruct/mod.rs (options parameter)
```

Phases 1-2 are independent and lower risk. Phase 3-4 is the larger effort. Phase 5 ties everything together.

## Tests

| Test | Verifies |
|------|----------|
| `test_import_sorting_groups` | std/external/crate grouping with blank line separators |
| `test_import_sorting_alphabetical` | Alphabetical within groups |
| `test_import_sorting_dedup` | Duplicate `use` paths emit once |
| `test_import_sorting_skip_flag` | `// kerai:skip-sort-imports` disables sorting |
| `test_derive_ordering` | `#[derive(Serialize, Clone)]` → `#[derive(Clone, Serialize)]` |
| `test_derive_ordering_skip_flag` | `// kerai:skip-order-derives` disables ordering |
| `test_suggestion_emitted` | Suggestion appears as `// kerai:` comment in reconstruction |
| `test_suggestion_dismissed` | Remove comment without fixing → not re-emitted |
| `test_suggestion_applied` | Remove comment and fix code → marked applied |
| `test_suggestion_re_emit` | Dismissed suggestion re-emitted when code changes and issue recurs |
| `test_suggestion_prefer_str_slice` | `&String` param detected |
| `test_suggestion_prefer_slice` | `&Vec<T>` param detected |
| `test_suggestion_non_snake_fn` | `fn myFunc` detected |
| `test_suggestion_unused_import` | Cross-file import analysis |
| `test_skip_all_flag` | `// kerai:skip` disables everything |
| `test_reconstruct_options` | JSON options parameter overrides flags |
| `test_roundtrip_with_suggestions` | Parse→reconstruct→parse cycle preserves suggestion state |

## Verification

```bash
# Check compilation
LC_ALL=C CARGO_TARGET_DIR="$(pwd)/tgt" cargo check

# Run clippy
LC_ALL=C CARGO_TARGET_DIR="$(pwd)/tgt" cargo clippy

# Run all tests
LC_ALL=C CARGO_TARGET_DIR="$(pwd)/tgt" cargo pgrx test pg17
```

## Future Extensions

- **Auto-fix mode**: For high-confidence rules, offer `// kerai:fix(rule_id)` to auto-apply the suggestion on next reconstruct
- **Severity escalation**: Track how often a suggestion is dismissed across files — if a rule is universally dismissed, consider disabling it instance-wide
- **Language-agnostic rules**: When Go/Python parsers are added, naming convention rules can be parameterized per language
- **Team conventions**: Import group ordering could be configurable (some teams put `crate::` before external)
- **Metrics**: Track suggestion acceptance rate per rule to measure which rules are useful
