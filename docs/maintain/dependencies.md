# Dependency choices and checks

Gazette uses established parsers, compilers and native libraries for their format-specific work. Its adapters constrain their inputs and connect their results to the publication model. This guide explains those choices, the limits that callers must preserve and the fixtures to revisit when a provider changes.

## Change a provider with its consumers

The [workspace manifest](../../Cargo.toml) and owning package manifests select exact direct versions and features. [Cargo.lock](../../Cargo.lock) records the complete resolved Rust graph and registry checksums; the generator locks record their own ecosystems. Use those files for current identity, rather than copying a package inventory into this guide. A target-filtered dependency graph omits other targets, and a successful build on the pinned toolchain does not establish support for older compilers or another operating system.

For a dependency change, inspect the selected features, upstream changes, notices and any retained patches, then run the provider's fixtures and its connected consumers below. Follow [build and test setup](../install.md) for the toolchain and executable bindings. Recheck affected targets when the lock, features, toolchain or native build environment changes. Gazette's native contract covers Linux and macOS; a configured workflow is not evidence of a completed run on either.

Input and output ceilings limit admitted work. Unless a provider-specific section says more, they do not bound every intermediate allocation or guarantee an execution deadline. A passing fixture establishes the behavior it exercises; it does not establish browser conformance, absence of vulnerabilities, licence compatibility or publication approval.

### Notices and distribution inputs

Gazette's packages inherit `AGPL-3.0-or-later` from the root manifest; [LICENSE](../../LICENSE) and [NOTICE](../../NOTICE) carry the repository's authoritative texts. The browser modules in `sdk/js/search`, `sdk/js/analytics` and `sdk/js/webmcp` are `Apache-2.0`, with their own `LICENSE`, `NOTICE` and SPDX line in each source file; the distribution configuration lists them as the product's exceptions. Preserve upstream notices in full, including incorporated components that a manifest's short licence expression does not describe.

[Distribution generation](../install.md#generate-distribution-notices) owns the commands, package identity rules and output checks. Generate notices and the source inventory from the locked inputs into a release or temporary directory. A source inventory and a binary SBOM answer different questions: a package can be present in the source graph without being included in a particular executable. Missing licence metadata remains `NOASSERTION`, not permission to infer a licence.

In particular, Rust wrapper metadata does not replace bundled native notices. `git2` and `libgit2-sys` declare MIT or Apache-2.0, while libgit2's [complete COPYING](https://github.com/libgit2/libgit2/blob/v1.9.7/COPYING) includes GPLv2 with a linking exception and incorporated PCRE2 and llhttp notices. The generated inventory verifies and retains that text separately. Bundled SQLite likewise needs its own provenance and compile options, distinct from its Rust wrapper and from any publication's adoption of SQLite. Cargo checksums do not pin operating-system libraries linked by the finished executable. Gazette's prohibition on handwritten unsafe Rust does not cover lower-level operations in dependencies such as rustix, libc or native libraries.

### Advisory checks

Run `python3 tools/check_dependencies.py --help` for the [dependency checker's](../../tools/check_dependencies.py) provisioning requirements, network use, input checks, reports and failure behavior. The [dependency workflow](../../.github/workflows/dependency-security.yml) supplies the selected scanners. Obtain a fresh receipt for the exact release inputs; old scanner results do not qualify a changed lockfile. Check native upstream advisories as well as language-package databases: the [Git provider](#git-objects-and-native-library-selection) explains why a clean Rust scan can miss a native-library issue.

## Fonts: reconstruct before inspecting

The [font adapter](../../adapters/fonts/Cargo.toml) uses Wuff's Brotli-backed WOFF2 reconstruction and `read-fonts` for SFNT table parsing. These providers own the font formats; Gazette checks the complete WOFF2 header, exact declared length, reserved field, table and decoded-size ceilings, and a TrueType or CFF flavor. Collections and unknown flavors are rejected. The reconstructed single font must contain parseable `head`, `maxp` and `name` tables.

Encoded and reconstructed digests are bound separately. Strict disclosure searches the complete reconstructed bytes under the aggregate decoded-data budget, rather than treating a successfully decoded font as an opaque passthrough. This establishes format and disclosure evidence, not glyph rendering, font sanitation, browser conformance or rights to use a font.

When changing either provider, run the [WOFF2 fixtures](../../adapters/fonts/tests/woff2.rs) and [connected font-resource fixtures](../../interfaces/launcher/tests/publication_page_resources/fonts.rs). Preserve the [fixture provenance](../../adapters/fonts/tests/fixtures/README.md): the neutral `valid-001.woff2` payload comes from the W3C WOFF2 WPT material retained by Wuff, with its original identity and notice. It is test data, never a publication font. Wuff and Fontations' applicable upstream notices remain distribution inputs.

## Documentation generators and Swagger conversion

External documentation generators are provisioned explicitly. The [portable workflow](../../.github/workflows/portable.yml) selects toolchains, consumes hash/integrity locks for Sphinx and TypeDoc, and builds gomarkdoc and go-swagger from the readonly Go module graph. The connected fixture remains download-free and disables Go network and toolchain fallback. It exercises Python/Sphinx, Rustdoc-compatible HTML, TypeScript/TypeDoc, Go/gomarkdoc and OpenAPI/Swagger in one publication assembly.

The runner binds regular executables using both their bytes and complete version output. Those same-run hashes detect selection changes; they are not an independent approval manifest. The reviewed locks establish source identity. Follow [generator maintenance](documentation-generators.md) for provisioning and the connected check, and [generator integration](../documentation-generators.md) for the runtime contract.

`swagger2openapi` supplies Swagger-to-OpenAPI conversion through `convert_obj`. Resolution, source-path inference and repairs are disabled: Gazette's bounded JSON/YAML parsers and offline validators own acceptance. The Mintlify wrapper corrects OAuth URL and form-media losses that otherwise survive schema validation. A valid output schema alone cannot establish conversion fidelity; preserve the [target profile](../mintlify-formats.md#openapi-and-swagger) and exercise both the [conversion fixtures](../../adapters/mintlify/tests/swagger.rs) and [generator fixtures](../../adapters/documentation/tests/swagger.rs).

The converter unconditionally enables Serde JSON's `preserve_order` feature and includes `serde_yaml_ng`/`unsafe-libyaml`, even though Gazette does not call its YAML/file parsing entry points. Review this unified feature graph when changing the dependency. Template contexts must normalize object order before both hashing and delivery; ordinary serialization and signed transport do not use that helper. Check exact context bytes and retained rebuilds through the application and publication fixtures, rather than accepting only the converter's local tests.

## Preview HTTP parsing

`httparse` supplies the default strict HTTP/1 request parser behind the [HTTP adapter](../../adapters/http/Cargo.toml). Gazette supplies bounded header storage and absolute request/response deadlines. `RoutePath` and the URL library continue to own path interpretation; an HTTP syntax parser does not decide publication routes.

The provider's safe API contains internal unsafe parsing/SIMD implementation. That boundary does not add handwritten unsafe code to Gazette, nor does it imply that the dependency contains none. On an upgrade, exercise malformed and bounded requests plus real loopback traffic in the [preview transport fixtures](../../adapters/http/tests/preview.rs). These checks cover Gazette's transport profile, not general server conformance or browser isolation.

## Markdown, heading IDs and templates

Comrak owns Markdown parsing and formatting through its public APIs. Gazette selects only `attributes`, with default features disabled; it does not select Comrak's CLI or syntax-highlighting bundle or copy its parser into the compiler. Preserve Comrak's complete `COPYING`, including inherited component notices. The [Markdown contract](../markdown.md) explains source analysis, normalized rendering and the source-position/backlink limits that consumers must keep.

The `slug` provider supplies ASCII transliteration for heading IDs using `deunicode`. It replaces a separately maintained slug grammar, while Gazette owns collision allocation and the relationship between IDs and navigation. Check the [Markdown adapter fixtures](../../adapters/markdown/tests), [heading fixtures](../../crates/gazette-compiler/tests/markdown_anchors.rs) and connected publication rendering after either provider changes, including Unicode text and collisions. Source, retained-fact and output limits remain logical ceilings; a benchmark at a recorded workload does not establish unrestricted capacity.

Tera renders captured templates with its default features disabled. Its [contract](../tera.md) and [adapter fixtures](../../adapters/tera/tests/render.rs) cover captured inheritance, escaping and limits. Preserve the distinction between captured template access and ambient filesystem lookup. Template execution remains trusted code.

## Interface providers and SDK composition

`rmcp` and Tokio supply MCP transport, `lsp-server`/`lsp-types` supply the language-server protocol, and `napi`, `napi-derive` and build-time `napi-build` expose the Node binding. The [MCP](../mcp.md), [LSP](../lsp.md) and [Node SDK](../../sdk/js/README.md) contracts connect those transports to shared Rust behavior.

Schemars projects schemas from the owning input and serialized-result types, including explicit failures and output limits. Configuration queries, native limits, source acquisition and selected paths keep their schemas beside those types; MCP must not grow a second field or variant catalogue. Its opaque handles use `getrandom` with bounded capacity and expiry. Handles identify transport state, not publication meaning. Native results are projected as structured JSON and matching JSON text under the result ceiling; the current operation inventory comes from [live discovery](../mcp.md#discovery-and-acceptance).

The Rust SDK has an adapter-free default. `authoring` selects the Markdown adapter for supplied-buffer analysis; `native-configuration` adds confined, caller-selected filesystem capture; `native-local` selects the full local composition. LSP uses `authoring` plus `native-configuration`, excluding Git, process, database, renderer, release-authority and deployment adapters. A feature selects capabilities without granting permission to use them.

Node-API registration is a foreign-function boundary. Only `gazette-node` uses `unsafe_code = deny` so that the provider's generated registration allowance can work; handwritten unsafe code remains denied, and other Gazette crates retain `forbid`. Test the actual addon on each supported Node host and target. Provider-generated code is not covered by a claim that Gazette's handwritten code is safe Rust.

Use the [MCP maintenance checks](mcp.md), [LSP protocol fixtures](../../interfaces/lsp/tests/protocol.rs), [SDK schema fixtures](../../sdk/rust/tests/schema_contracts.rs) and Node SDK checks for a provider change. Recipe fixtures reuse `git2` and `jsonschema` as development dependencies for disposable repositories and schema/native parity. Supplied release verification reuses the existing verifier and frozen signed fixtures; interface exposure adds no signing provider, private-key access or release effect. Catalogue presence alone does not establish external-client behavior.

## Exact JSON, schemas and release signatures

The shared Serde JSON decoder enables `arbitrary_precision` and `raw_value`. It preserves exact numeric literals and rejects duplicate decoded keys, including cases involving escaped spellings, without treating private Serde token names in user objects as special values. Borrowed raw slices reuse the native grammar with explicit retained-depth and caller-owned byte bounds. Notebook parsing and Mintlify's TypeDoc validation share this decoder; do not introduce an adapter-local replacement.

`jsonschema` uses `arbitrary-precision` with default features disabled and an always-refusing external retriever. Notebook metadata cannot supply a schema or retriever. The retained nbformat schema and BSD-3-Clause notice have their own [upstream provenance](../notebooks.md#upstream-schema-provenance). OpenAPI validation composes the official offline document/dialect validators at typed positions, as recorded in [schema provenance](../mintlify-formats.md#schema-provenance). Structural validation neither runs notebook cells nor establishes rendering correctness.

Release verification composes `serde_jcs`, `ed25519-dalek` with only `std`, and Base64. Duplicate-safe decoding precedes typed shape validation and JCS. The exact-number predicate rejects non-integral spellings and integers outside the JCS-safe range before binary64 conversion; Serde otherwise normalizes `-0` at its value boundary. Input byte/node/depth limits bound supplied work, and the limited writer separately bounds final bytes, but JCS may allocate internal object-member buffers. [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785.html) defines the canonicalization format.

Dalek's fixed `verify_strict` path requires canonical compressed public-key spelling and strict Base64 round trips. No batch, prehash, legacy-compatibility or algorithm-negotiation mode is selected. Gazette exposes no signing or private-key operation. The [release contract](../releasing.md) owns exact signed bytes, scope/time checks and caller-owned acquisition; successful verification alone does not prepare, authorize or deliver a release.

On a provider or feature change, run the [shared JSON fixtures](../../crates/gazette-core/tests/json.rs), [notebook fixtures](../../crates/gazette-compiler/tests/notebook.rs), affected Mintlify validators and [release authorization fixtures](../../crates/gazette-compiler/tests/release_authorization.rs). Preserve exact-number, duplicate-key and safe-integer cases as well as valid signed fixtures.

## Raster decoding, metadata and transformations

The [raster manifest](../../adapters/raster/Cargo.toml) separates pixel inspection, metadata inspection and image transformation. Preserve that separation when changing a codec: successful decoding does not sanitize metadata, certify browser behavior or authorize disclosure.

### Stored PNG and JPEG

The notebook inspection path uses `png` and `zune-jpeg`/`zune-core`, with Zune defaults disabled and only `std` enabled. It uses neither the `image` facade nor SIMD, a decoder process or native file access. The JPEG release-candidate pin was selected because the previous candidate's progressive-component handling was implicated in an upstream [malformed-image panic report](https://github.com/etemesi254/zune-image/issues/409). Its corrected component iteration and sampling maxima need checking when replacing it. The report's attached fuzz samples were not successfully retrieved, so source inspection does not establish that those samples were reproduced. The selected Zune core also avoids the rejected yanked candidate.

PNG checks checksums, expands packed/indexed pixels without discarding sixteen-bit precision, rejects animation and decodes through the end chunk. JPEG applies strict options and dimension ceilings before header parsing, decodes pixels and checks the reported decoded byte count. Both check encoded bytes before constructing the provider and separately bound the output buffer. [Core raster limits](../../crates/gazette-core/src/raster.rs) own the defaults; notebook transport also respects its caller's source ceiling. Per-image limits do not bound total memory, aggregate images or execution time.

Notebook Base64 transport requires strict padding and trailing bits; only CR/LF line endings are removed from joined strings. Evidence binds the exact original bytes, format, dimensions, encoded length, digest and decoder/profile identity before constructing the fixed data URI. The compiler calls a pure callback; application/SDK composition supplies the decoder implementation.

Exercise [real decoding](../../adapters/raster/tests/inspection.rs) and [connected notebook images](../../interfaces/launcher/tests/publication_render/notebook_images.rs), including malformed/truncated input, MIME mismatches, PNG CRC/end-chunk and animation rejection, limits, retained-byte identity, warm-cache replay and changed-provider refusal. Mock callbacks establish transport binding only. The [notebook contract](../notebooks.md) owns the remaining publication limits.

### Metadata and WebP

PNG metadata inspection uses `flate2` with `zlib-rs`. Native counters and `StreamEnd` establish exact compressed-member closure; bounded inflation and metadata fields consume shared scan budgets. Gazette's [closed PNG profile](../output-disclosure.md#raster-metadata-in-policy-checks) adds checks for malformed keywords, unsupported members and trailing compressed/container data that the provider may tolerate.

JPEG metadata inspection reuses the strict pixel decoder after bounded marker/container inspection. It admits thumbnail-free JFIF and UTF-8 comments only. EXIF, ICC, XMP and animation remain unsupported by these metadata profiles. Check [PNG metadata](../../adapters/raster/tests/metadata.rs), [JPEG metadata](../../adapters/raster/tests/jpeg_metadata.rs) and the [connected raster disclosure fixtures](../../interfaces/launcher/tests/publication_render/probe/raster.rs) when changing this path.

`image-webp` supplies single-image decoding after Gazette checks the closed RIFF container, dimensions and bounds. Evidence remains tied to the original bytes. Its [memory-limit setting](https://docs.rs/image-webp/0.2.4/image_webp/struct.WebPDecoder.html) does not cover all internal allocations; the output buffer has a separate Gazette limit. Metadata-bearing, animated and unknown containers are unsupported. Compare the [WebP fixtures](../../adapters/raster/tests/metadata/webp.rs) with the [container specification](https://developers.google.com/speed/webp/docs/riff_container) when the provider changes.

### Captured image transformations

Transformation selects `image` with defaults disabled and only PNG enabled. It reuses the strict Zune JPEG decoder and uses `jpeg-encoder` with only `std` to emit JPEG. Image's JPEG feature remains disabled because its stable decoder constraint conflicts with the selected prerelease and would introduce a different decoder path.

Preserve the encoder's `(MIT OR Apache-2.0) AND IJG` notices, including its IJG-derived implementation acknowledgement, alongside Image and its transitive components' notices. Decoding, transformation, emission and disclosure approval have separate roles in the [image resource contract](../page-resources.md#captured-image-transformations). Check [native transformations](../../adapters/raster/tests/transform.rs) and [connected image resources](../../interfaces/launcher/tests/publication_page_resources/images.rs) against current source.

## Prose language and authored dates

`oxilangtag` owns BCP 47 well-formedness and subtag case normalization for `LanguageTag`. Gazette uses the provider rather than maintaining another language-tag grammar. Its 255-byte input ceiling is an application bound, not the standard's maximum. Parsing does not consult a mutable registry or identify the language of the prose. Deprecated tags, private-use meanings, locale fallback and translation identity remain unchanged. Preserve the provider's Pyfisch/Tpt copyright and MIT notice.

Chrono owns calendar/time parsing for `EditorialDate`, with defaults disabled and only `std` enabled. The selected path has no clock, timezone database, local-time or WebAssembly bindings. Gazette's lexical guard rejects flexible separators, unknown zero offsets, leap seconds and fractions beyond nanosecond precision that the provider would accept or normalize. It also limits the common output profile to years 0001–9999 and offsets within ±14:00, preserving valid [XML Schema dateTime](https://www.w3.org/TR/xmlschema-2/#dateTime-timezones) output.

The [authored contract](../content-model.md#authored-description-and-chronology) retains date-only precision and explicit nonzero offsets. RFC 3339's [unknown offset](https://www.rfc-editor.org/rfc/rfc3339.html#section-4.3) `-00:00` must not become `Z`. There is no custom calendar arithmetic, inferred date, clock default or timezone lookup.

Check [core editorial fixtures](../../crates/gazette-core/tests/editorial.rs), [authored projection](../../crates/gazette-compiler/tests/authored_projection/editorial.rs) and connected outputs after either provider changes. Cover private use, grandfathered language forms, casing, invalid syntax and the byte ceiling, plus invalid calendars, precision/offset boundaries, canonical serialization, source coordinates and text budgets.

## Decoding text for disclosure checks

These parsers expose decoded text to the application's comparator. A finding is provisional until the complete retained input succeeds; an early match must not turn an incomplete or malformed document into accepted evidence. The [output probe contract](../output-disclosure.md#private-output-disclosure-probe) owns supported formats, exclusions and aggregate budgets.

### Encoded candidates

The one-layer encoded operation reuses `url`, `percent-encoding` and Base64. URL parsing owns scheme preprocessing and fragment-free serialization. Percent decoding preserves bytes and does not interpret `+` as a form separator. The selected forgiving Base64 profile normalizes bounded whitespace/padding before delegating bit assembly; it is distinct from strict notebook and signature transport.

The metadata guard supports fewer forms than general MIME parsing and refuses unsupported metadata/charsets without repair. Input and exact decoded length are checked before payload allocation; URL/metadata scratch remains input-bounded. Application-owned aggregate depth, count, byte and comparison budgets govern nested scans. Unknown binary payloads remain unqualified. Check the [encoded fixtures](../../crates/gazette-compiler/tests/encoded.rs) against the [candidate contract](../output-disclosure.md#encoded-candidates-and-embedded-coverage).

### HTML

`html5ever` provides token decoding, not a full DOM or browser. Gazette explicitly selects raw/RCDATA states and rejects every parse error, including duplicate attributes the provider would otherwise drop. Callbacks must return `Continue` for errors/character tokens, so the sink latches its first failure, suppresses later observations and stops after the bounded feed. EOF is finalized only after successful feeds and complete input consumption.

The selected provider applies `discard_bom` on every feed. Gazette disables that option and removes exactly one stream-leading BOM after checking original bytes, preserving interior U+FEFF across chunks. Attribute deduplication may be quadratic within a tag. Input, buffer, callback and decoded-byte ceilings constrain work without being a CPU or allocation sandbox. Revisit these assumptions in the [HTML fixtures](../../crates/gazette-compiler/tests/html.rs) when upgrading.

### CSS

`cssparser` is reused directly from the Lightning CSS dependency graph. Its tokens supply decoded identifiers, strings and unquoted URLs; Gazette has no second CSS grammar. Original token slices and actual closing delimiters must still be checked because the provider tolerates implicit EOF termination of strings, comments, URLs and blocks. Text decoding remains separate from property validation, URL decoding and runtime evaluation. External CSS and HTML style channels share the application consumer.

Visited-token, nesting and delivered-byte ceilings bound inspection. Native `parse_nested_block` cleanup continues after consumer failure using an iterative heap stack; that cleanup may exceed visited-token/depth ceilings while remaining bounded by the already checked input. Check complete-input refusal and decoding in the [CSS fixtures](../../crates/gazette-compiler/tests/css.rs), and preserve the provider's MPL-2.0 notice.

### XML

Quick-XML, with optional features disabled, provides a lexical guard before `xml` performs namespace-aware XML 1.0 parsing. Both are needed for the selected versions: `xml` silently retains the first duplicate namespace declaration and excludes those declarations from its attribute ceiling. Quick-XML's checked attribute iterator rejects duplicates and counts all declarations. It does not replace the semantic parser's document grammar, reference/newline normalization or expanded-name duplicate-attribute checks.

Gazette checks reserved namespace bindings using decoded values and rejects actual DTD events before semantic parsing. The guard retains explicit namespace-prefix coordinates so unused declarations can be inspected without inventing observations from implicit bindings. Declarations must name exactly XML 1.0 and, if present, UTF-8. Gazette checks original bytes as bounded UTF-8 before removing one leading BOM, fixes the semantic encoding to UTF-8 and disables multiple roots, replacement-character repair and invalid-encoding tolerance. It supplies no external entity resolver or native reader.

Input, lexical/semantic events, depth, names, attributes, values and decoded output have bounds; native checks may overshoot by a token/reference. Errors do not expose source-bearing provider messages. Revisit duplicate namespace declarations, full-document consumption and those resource boundaries in the [XML fixtures](../../crates/gazette-compiler/tests/xml.rs).

## Git objects and native library selection

`git2` selects vendored libgit2 with default features disabled and neither SSH nor HTTPS enabled. This supports exact local object access without bringing network transports into that binding. The selected libgit2 includes the delta-result size check before allocation; the earlier library lacked the upstream [local-pack allocation fix](https://github.com/libgit2/libgit2/security/advisories/GHSA-27m5-gxxh-x79j). Disabling transports does not remove local native parsing or make that fix unnecessary.

The [provider fixture](../../adapters/git/tests/provider.rs) calls the linked library and checks its exact version, vendored identity and disabled transports. Cargo metadata alone cannot establish which native library was linked. Do not use `LIBGIT2_NO_VENDOR` for this build: the fixture rejects system substitution even at the expected version. Recheck [source/worktree fixtures](../../adapters/git/tests/worktree.rs) and connected source capture when changing the binding.

The upstream delta ceiling is `UINT31_MAX`, not Gazette's per-file or metadata budget. Logical object-size checks before requested reads do not bound every C-library allocation. A clean RustSec scan also cannot establish complete native advisory coverage: the allocation issue was known upstream while absent from the RustSec snapshot used during the correction. Keep checking native advisories; neither result establishes hostile-pack isolation.

The [local commit path](../releasing.md) uses an in-memory index, raw blob/commit writes and the native commit-buffer encoder. Creation and read-back use the same encoder and reject unexpected extra headers. This path does not invoke worktree/index filters, signature discovery, hooks, checkout, fetch or push. Detached object creation is distinct from durable release preparation; exercise the [release fixtures](../../adapters/git/tests/release.rs) and connected release consumers for changes to it.

## SQLite migrations

`rusqlite` supplies the bundled SQLite adapter, and `rusqlite_migration` supplies one migration-version owner and transactional initialization. The [store guide](../store.md) explains their lifecycle and recovery role. Check the [connection](../../adapters/sqlite/tests/internal/connection.rs) and [writer fixtures](../../adapters/sqlite/tests/internal/writer.rs) when changing either provider. Keep SQLite's native provenance, wrapper metadata and build options separate in distribution evidence; an adapter test does not establish hardware durability.

## Browser modules: compilation and validation

SWC owns parsing, TypeScript transformation, code emission and bundling for the [JavaScript adapter](../../adapters/javascript/Cargo.toml). Gazette supplies captured-only loaders/resolvers and logical filenames. It does not inherit ambient filesystem/package lookup or substitute empty modules for unsupported assets. Exact compiler component versions and features belong in the manifests and lock, not in the adapter's short profile label.

SWC does not check every JavaScript lexical early error, as the duplicate-constant case demonstrates. OXC's parser and semantic analysis therefore validate input and emitted-module grammar/early errors; their AST is discarded after validation. SWC remains the only transform/bundle implementation. The compiler's disclosure probe reuses OXC syntax nodes for native strings, identifiers and cooked template chunks, feeding the existing comparator without exposing the AST in the publication model.

Grammar validation and TypeScript erasure are not TypeScript type checking, browser compatibility downleveling or a hostile-program sandbox. Source/output ceilings do not bound every provider allocation or guarantee a deadline. Check the [adapter compilation fixtures](../../adapters/javascript/tests/compile.rs), [JavaScript probe fixtures](../../crates/gazette-compiler/tests/javascript.rs) and [connected module fixtures](../../interfaces/launcher/tests/publication_page_resources/modules.rs). The connected synthetic ESM execution check requires Node as an explicit test prerequisite and fails if it is unavailable; Node is not a Gazette runtime dependency. Preserve SWC's Apache-2.0 and OXC's MIT notices with the complete transitive inventory.

## Sass compilation and CSS validation

Grass owns Sass parsing and compilation from retained input bytes. Gazette uses its public AST to admit decoded import literals before extension probing, which would otherwise substitute `.scss` for unsupported suffixes such as `.scss?raw`. The compilation API cannot accept that AST, so each consumed file incurs one cached admission parse in addition to compilation. This keeps one Sass grammar and emitter while allowing Gazette to enforce captured resource access.

The selected `grass_compiler` is the retained `parser-repairs.2` source. Its [patch account](../../adapters/styles/vendor/grass_compiler-0.13.4/GAZETTE-PATCH.md) preserves the upstream archive identity, complete delta and licence. The repairs restore parser state after declaration/selector fallback and terminate malformed-input hang/panic paths. Generated notices identify this modified tree separately from the original registry archive. Do not substitute the upstream archive checksum for the patched tree's identity.

Grass can misclassify transitive filesystem errors as span-based Sass errors and panic. The adapter retains the first refused-input diagnostic, stops later captured reads and supplies only a rejection directive through that callback; the retained failure wins before any result is accepted. It also catches provider panics across import admission and compilation, returning the stylesheet compile-error category. A failed call produces no artifact, and the next request gets a fresh compiler.

Lightning CSS, with only `visitor` enabled, validates emitted CSS and the closed-resource profile without rewriting output. The policy validates Grass's result rather than implementing another Sass grammar: for example, `calc(90%)` can become `90%`, which is admitted only if the emitted descriptor is within the typed whitelist. The locked `parcel_selectors` version must remain compatible with `cssparser`; the rejected upgrade selected a different parser generation and failed native compilation. Preserve Lightning CSS, selectors and cssparser's MPL-2.0 notices alongside Grass's MIT notice.

Input and artifact ceilings do not bound provider intermediate allocation or execution time. Dynamic `sass:meta` loading and unresolved CSS resource references remain excluded. Declaration-only imported fragments are also unsupported by the selected Grass provider; the parser repairs do not close that gap. Nested imports of complete rules are checked separately.

Run the [stylesheet fixtures](../../adapters/styles/tests/compile.rs) when changing any part of this chain. They include the native declaration-only reference control, captured import refusals, complete nested rules, selector fallback with literal grid spans, retained first failure, panic recovery and malformed-input checks in a time-bounded subprocess. Preserve both successful compilation and failure behavior; neither establishes complete Sass capability or browser isolation.

Back to [Contributing](../../CONTRIBUTING.md).
