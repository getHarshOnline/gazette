# Retained Grass compiler parser repairs

This directory contains only the published `grass_compiler` 0.13.4 crate, with bounded parser repairs. Other Cargo dependencies continue to use their exact locked registry sources. The public `grass` 0.13.4 API remains unchanged.

- Upstream: https://github.com/connorskees/grass
- Published package: https://static.crates.io/crates/grass_compiler/grass_compiler-0.13.4.crate
- Original package SHA-256: `2d9e3df7f0222ce5184154973d247c591d9aadc28ce7a73c6cd31100c9facff6`
- Upstream VCS revision recorded in the package: `e0bb9e2eabfc3a58e42b03089cd7b22c68d09d0b`, `crates/compiler`.
- Licence: MIT. The original package declares MIT but omits its licence file; `LICENSE` is retained unchanged from https://raw.githubusercontent.com/connorskees/grass/e0bb9e2eabfc3a58e42b03089cd7b22c68d09d0b/LICENSE (SHA-256 `7d0010e1e803b6920660f1a04f333888722943d6647571111ab1f3cd46d9cc47`).

## Exact changes

`GAZETTE-parser-repairs.patch` is the complete delta to upstream implementation bytes. In `parse_declaration_or_buffer`, save parser context before speculatively parsing a declaration value. If that attempt fails and the input is retried as a selector, restore that context together with the existing token-cursor rewind. A valid nested selector such as `a:not(:where(.action a))` must not leave `IN_PARENS` set for later declarations and silently turn literal CSS grid spans into Sass arithmetic.

No CSS rewrite, grid-specific exception, selector restriction, Sass arithmetic change, resource admission change or host filesystem access is added. `adapters/styles/tests/compile.rs::nested_selector_fallback_does_not_turn_later_grid_spans_into_division` exercises the captured resource compiler and verifies the selector, subsequent grid column/row/area spans, and intentional parenthesized/multiplied Sass arithmetic. It fails against unmodified 0.13.4 before this patch.

The source-distribution evidence configuration records every retained file hash. Cargo binds this directory explicitly through `[patch.crates-io]`; the lock records a local patched dependency, not the original registry archive checksum. Original archive provenance above must not be presented as a checksum for the modified tree. No upstream submission, external publication or release approval is implied.

The second repair revision (`parser-repairs.2`) also terminates indented-syntax comments at EOF,
returns an ordinary error for a selector without a compound selector, and rejects unsupported
`@-moz-document` CSS with a parse error. These previously looped or reached unfinished panic paths.
The captured-compiler regression `malformed_stylesheets_fail_without_panicking_or_hanging`
checks all three in a time-bounded subprocess. Valid CSS semantics are unchanged.
