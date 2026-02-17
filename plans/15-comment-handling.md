# Plan 15: Improved Rust Comment Handling During Ingestion

*Depends on: Plan 02 (Rust Parser), Plan 03 (Source Reconstruction)*
*Status: Complete*

## Goal

Fix the comment pipeline so that regular comments (`//`, `/* */`) are correctly extracted, grouped, classified by placement, excluded from string literals, and preserved through reconstruction. Add a source normalizer for consistent parsing. Strengthen the round-trip guarantee from Plan 03 to include comments.

## Problem

The original comment extractor (Plan 02) had several issues:

1. **No source normalization** — BOM, CRLF, trailing whitespace, and excessive blank lines weren't handled before parsing
2. **False positives** — `//` inside string literals was incorrectly extracted as comments
3. **No grouping** — Consecutive `//` lines were stored as separate nodes instead of a single block
4. **Naive matching** — Forward-only `find_nearest_node_after_line()` couldn't distinguish above, trailing, between, or EOF comments
5. **No placement metadata** — No way to know if a comment was above a function, trailing on a line, between items, or at end-of-file
6. **Comments lost in reconstruction** — `prettyplease` formatter stripped regular comments via `syn::parse_file()`

## Approach

Pure-Rust normalizer + restructured comment pipeline with grouping, placement classification, string-literal exclusion, and smarter matching. Comment-preserving formatter for reconstruction.

## Deliverables

### 15.1 Source Normalizer

`src/parser/normalizer.rs` — Pure function `normalize(source: &str) -> String`:

1. Strip UTF-8 BOM (`\u{FEFF}`)
2. Normalize CRLF → LF
3. Strip trailing whitespace per line
4. Collapse 2+ consecutive blank lines → 1
5. Ensure exactly one trailing `\n`

Called at the top of `parse_single_file()` before both `syn::parse_file()` and `extract_comments()`.

### 15.2 String Literal Exclusion Zones

`collect_string_spans(file: &syn::File) -> Vec<(usize, usize)>` — Uses `syn::visit::Visit` to walk the parsed AST and collect `(start_line, end_line)` spans for all string literals (`Str`, `ByteStr`, `CStr`). Lines inside multi-line string literals are excluded from comment extraction.

### 15.3 Comment Grouping

`group_comments(comments: Vec<CommentInfo>) -> Vec<CommentBlock>` — Merges consecutive `//` comments (same column, adjacent lines, same doc/non-doc type) into `CommentBlock` nodes. Block comments (`/* */`) remain standalone.

New types:
- `CommentPlacement` — `Above`, `Trailing`, `Between`, `Eof`
- `CommentBlock` — Grouped comment with start/end lines, placement, style metadata

### 15.4 Placement-Aware Matching

`match_comments_to_ast(blocks: &mut [CommentBlock], nodes: &[NodeRow]) -> Vec<Option<String>>` — Classifies each comment block:

| Placement | Condition | Edge |
|-----------|-----------|------|
| **Trailing** | Same line as an AST node | `documents` → same-line node |
| **Above** | Directly above next node (no gap) | `documents` → next node |
| **Between** | Gap before next node + previous node exists | `documents` → next node |
| **Eof** | No AST node after comment | None (parented to file) |

### 15.5 COMMENT_BLOCK Kind

`kinds::COMMENT_BLOCK` — Used for multi-line grouped `//` comments. Single-line `//` and `/* */` use `kinds::COMMENT`.

### 15.6 Comment Node Metadata

```json
{
    "start_line": 10, "end_line": 12, "col": 1,
    "placement": "above", "style": "line", "line_count": 3
}
```

Edge metadata: `{"placement": "above"}` on the `documents` relation.

### 15.7 Comment-Preserving Reconstruction

Updated `assembler.rs`:
- Children queried in position order (line-number-based for both items and comments)
- Comment nodes emitted directly when encountered (above, between, eof)
- Trailing comments appended to their target item's last line
- No duplication: comments either appear as children OR via edges, not both

Updated `formatter.rs`:
- Splits source into alternating comment/code segments
- Formats code segments independently through prettyplease
- Preserves comment segments verbatim
- Falls back to normal formatting when no regular comments present

### 15.8 Position Normalization

Top-level item positions normalized from array indices to `span_start` line numbers after AST walking, so items and comments interleave correctly in the same number space. Also fixed `walk_impl` to include span_start/span_end from the `impl` keyword span (was passing `None`).

## Files Modified

| File | Change |
|------|--------|
| `src/parser/normalizer.rs` | **New.** Source text normalizer with 10 unit tests |
| `src/parser/comment_extractor.rs` | Restructured: CommentBlock, CommentPlacement, grouping, exclusion zones, 7 unit tests |
| `src/parser/kinds.rs` | Added `COMMENT_BLOCK` constant |
| `src/parser/mod.rs` | New parse flow, `match_comments_to_ast()`, position normalization |
| `src/parser/ast_walker.rs` | Fixed `walk_impl` span_start/span_end |
| `src/reconstruct/assembler.rs` | Comment-aware reconstruction with position-ordered children |
| `src/reconstruct/formatter.rs` | Comment-preserving segmented formatting |
| `src/lib.rs` | 8 new `#[pg_test]` tests |

## Tests

| Test | Verifies |
|------|----------|
| `test_comment_grouping` | 3 consecutive `//` → 1 `comment_block` node |
| `test_comment_placement_above` | `// helper` above `fn foo()` → placement=above, documents edge |
| `test_comment_placement_eof` | Comment at end of file → placement=eof, no documents edge |
| `test_comment_not_in_string` | `"// not a comment"` not extracted |
| `test_normalization_crlf` | CRLF source parses correctly |
| `test_normalization_blank_lines` | Multiple blank lines between fns → collapsed |
| `test_roundtrip_with_comments` | Parse+reconstruct preserves above comments |
| + 17 unit tests | Normalizer (10) and comment extractor (7) |

## Verification

```bash
LC_ALL=C CARGO_TARGET_DIR="$(pwd)/tgt" cargo pgrx test pg17
# 181 tests pass, 0 failures
```
