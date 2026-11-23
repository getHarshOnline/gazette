# Output disclosure checks

Check generated files for private values before preparing a release. Start with a [local render](local-rendering.md) or an output tree you have verified independently. Gazette scans those files and reports matches. It never edits the files.

| Your integration needs | Use |
| --- | --- |
| Find specific private strings in a local render or pinned output tree | [Direct CLI inspection](#direct-cli-inspection), or [the Rust probe](#private-output-disclosure-probe) |
| Refuse unsupported formats as well as literal matches | [Required supported-profile check](#required-supported-profile-check) |
| Match local paths, endpoints and other prohibited values by their purpose | [Typed disclosure policy](#typed-disclosure-policy) |
| Run a publication owner's external checker over the files | [Configured policy checks](policy-checks.md#check-the-actual-rendered-bytes) |

Keep requests and reports private. The paths, rule names and diagnostics can disclose the values you are trying to keep out of the build. Read both coverage and findings before making a release decision. A scan does not make a program safe to execute; use the [trusted-code model](architecture.md#trusted-code-and-saved-results) when selecting templates and external programs.

## Direct CLI inspection

Scan a local render for a literal marker without writing it to a directory.

::::steps

:::step "Create the private scan request"

Start with your complete configuration in `workspace.toml`. Encode it and a rule as JSON:

```sh
python3 -c 'import json,sys; json.dump({"format":"gazette-disclosure-probe-input/1","workspace":sys.stdin.read(),"rules":[{"id":"rule:private-marker","literal":"PRIVATE-CANARY"}]},sys.stdout)' \
  < workspace.toml > private-probe.json
```

Edit `private-probe.json` and replace `PRIVATE-CANARY` with the value to look for. Keep the value on stdin rather than in command arguments.

:::

:::step "Render and scan"

Replace `site:one` with your configured site. Use the same page, alias and graph settings as the build you want to inspect.

```sh
gazette render-site --local --site site:one \
  --model-output model.json --probe-disclosure < private-probe.json
```

:::

:::step "Read findings and coverage"

Read the JSON result on stdout. It reports what matched and what was inspected. Zero matches can occur when a file format was not decoded. Keep the actual report before using it for another check: a handled closed output pipe can produce exit zero without delivering the report.

:::

::::

The encoded request has this shape:

```json
{
  "format": "gazette-disclosure-probe-input/1",
  "workspace": "complete workspace TOML encoded as one JSON string",
  "rules": [
    {"id": "rule:private-marker", "literal": "PRIVATE-CANARY"}
  ]
}
```

The `workspace` string is the complete configured TOML, with quotes and newlines encoded as JSON. Do not substitute a filename for it. Keep this request private and pass rules through stdin rather than command arguments.

Unknown versions or fields, duplicate fields or rule IDs, empty rules or literals, and excess rule budgets fail before source reads. `--max-source-bytes` counts the full JSON on stdin before decoding, including escaping. The default is 64 KiB. The embedded TOML and scanner have separate limits.

Both probe commands accept `--max-probe-encoded-input-bytes`, `--max-probe-decoded-bytes`, `--max-probe-encoded-depth`, `--max-probe-encoded-values`, `--max-probe-encoded-bytes`, `--max-probe-work-bytes` and `--max-probe-html-tokens`. The last counts tokenizer callbacks per HTML input, including HTML embedded in Markdown, with a default of 100,000. Increase a limit when the files require it. The report includes the resource digest for that choice. A changed scan budget does not change publication rules. Invalid depth fails before any source read.

Gazette compiles once and scans the rendered bytes. It does not scan an unrelated output directory or reopen templates. `--output-parent` conflicts with probing before input is read. An error returns no successful JSON report. Without `--probe-disclosure`, the command accepts raw TOML as usual.

For an already retained output tree, use `gazette probe-output --tree PATH --tree-identity '{"format":"gazette.inventory.v1","sha256":"..."}'` with the actual independently retained tree identity. Its private stdin is:

```json
{"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:private-marker","literal":"PRIVATE-CANARY"}]}
```

This mode requires no workspace, recipe or rebuild: the existing confined reader captures and verifies the exact pin before `build::probe_disclosure` scans it. No producer media map is inferred for imported bytes; suffix selection remains explicitly weaker evidence. Rust callers use that same free operation with a captured tree and its available producer map. A declaration for a non-file is rejected, but merely supplying a map does not authenticate its source. No scan rewrites files, stages output, repairs a mismatched tree or certifies publication.

## Private output disclosure probe

Use `gazette_sdk::build::probe_disclosure` to inspect retained output with the SDK's Markdown, raster and font decoders:

```rust,ignore
let site = rendered.site();
let report = gazette_sdk::build::probe_disclosure(
    site.output(), site.media_types(), &rules, limits,
)?;
```

For the application’s default decoder set, call `rendered.site().probe_disclosure(&rules, limits)`. It scans the current output without reading sources or compiling again.

The same SDK composition is used by `check_disclosure` and `check_disclosure_policy`. At the application layer, `RenderedSite`'s ordinary convenience methods do not select those optional decoders. An application embedding that layer supplies `DisclosureInspectors` to the corresponding `*_with_inspectors` method. Missing Markdown parsing is an error; missing raster or font inspection retains unsupported coverage in a diagnostic and refuses a required check. Do not infer decoder coverage from a zero-findings result.

Rules are a nonempty `BTreeMap<Key, String>` of private identifiers to nonempty, case-sensitive literal needles. They are not a publication policy or a source owner's ruling. `DisclosureProbeLimits::validate_rules` checks the same rule shape/ceilings and returns its digest before any acquisition or rendering is needed. The probe revalidates rules rather than relying on a caller's earlier check. The operation scans every exact inventory path and every raw file payload, including unreferenced assets and compiled resources. It never rereads native sources, rewrites prose, removes a file or creates a stage.

## Probe report and comparison limits

`DisclosureProbe` retains the exact output reference and serializes its tree identity, probe version (`gazette-disclosure-probe/16`), canonical/caseless filename profile, compact rule-set digest, work charge, per-file decoder coverage and deterministic findings with their digest. Diagnostic traversal has status `partial-evidence`, including when the findings list is empty. The separately owned mandatory traversal records `required-supported-profile-evidence` only after unknown classes, unsupported decoding, findings and exhausted bounds have failed closed. Findings contain path, rule ID and representation, not matching payload snippets. Paths and rule IDs may themselves be confidential: this is private operator evidence, not a public artifact.

`DisclosureProbeLimits` bounds rule count, encoded rule bytes, output entry count, comparison work, distinct findings and complete findings-encoding bytes. Each literal comparison charges the conservative product of haystack bytes and needle bytes before searching, including decoded JSON strings, HTML, XML, CSS and JavaScript channels. Rules are encoded through the same bounded JSON writer used by the render model. The JSON parser may allocate an escaped string, bounded by the already charged raw input, before charging its decoded comparison. Any overflow or exhausted ceiling returns no report; findings are never truncated. Every selected ceiling is projected under `gazette-disclosure-resources/2` and hashed into `resource_limits_sha256`. Preparation qualification compares that digest with the independently declared resource expectation. Choose the limits before scanning and obtain their digest with `DisclosureProbeLimits::resource_limits_sha256()`; the associated `RESOURCE_PROFILE` names the projection. The local preparation and direct qualification paths compare those chosen limits before reading sources or creating effects.

Preparation qualification binds the scanner, `gazette-encoded/1` decoder, rule digest and structured-decoder profiles as `gazette-immutable-output-technical/0.1`. It requires those exact versions and the declared resource-limit digest; a scan completed with different limits is not interchangeable. Authored and generated-documentation certificates bind the same exact expectation, and history is checked again against that expectation when reopened. History without a technical certificate keeps the default-limit requirement. When checking recorded raster coverage, qualification also verifies the independently selected PNG, JPEG or WebP metadata-decoder profile.

The same certificate requirements bind `gazette-zero-output-exclusion/0.1` with `removed=[]`. This profile performs no file deletion and admits no nonempty, omitted, alternate or historical compatibility state. A future nonzero generated-artifact allowlist requires its own workspace-owned policy contract; the Engine does not infer one from filenames or scanner findings.

The resource profile binds retained bytes, entries, parser input, tokens, events, nodes, depth, decoded bytes/values, recursion and comparison work. It does not record elapsed time, so the same input is checked independently of machine load. These are logical work limits, not OS CPU or memory isolation.

A diagnostic report is not disclosure certification. Exact filenames are not normalized; evaluated JavaScript values, unsupported embedded formats and unqualified binary metadata remain outside its coverage. XML or JSON decoding does not qualify a value's embedded format. The supported-profile and typed-policy checks supply additional fail-closed contracts; the diagnostic alone does not bind complete owner-approved rules or certification/release consumption. It introduces no CLI certification command or publication permission.

## Required supported-profile check

The Rust application/SDK `check_disclosure` operation (also available on `RenderedSite`) uses the same immutable traversal as `probe_disclosure`. It refuses raw-only files, unknown/binary data payloads, non-UTF-8 percent-decoded text and literal-rule findings. No file is rewritten, removed or staged.

It also refuses native HTML data-script and `on*` attribute contexts, nonempty external-script bodies, XML processing instructions, and namespace-resolved SVG script/style/foreignObject or XHTML elements, including empty ones. Unrelated XML namesakes do not acquire SVG semantics. Empty recognized external-script bodies have no inline payload to inspect; accepting that empty channel does not acquire or qualify the referenced resource.

Success returns `required-supported-profile-evidence`, making the mandatory fail-closed traversal distinguishable from a diagnostic probe in retained evidence. That report is still not a certificate: only the separate preparation qualification step can bind it to exact preparation and policy evidence. Executable URL interpretation, evaluated JavaScript and regular expressions, source-map embedded source-language decoding, filename normalization, authenticated media declarations, nonzero generated-artifact allowlists and policy/release binding remain outside this check. Findings retain private typed coordinates; ordinary error text exposes only a category or count. This operation does not add a CLI/MCP command or grant publication permission.

## Typed disclosure policy

`check_disclosure_policy` applies an explicit `DisclosurePolicy` through the same immutable traversal and decoder owners. Unlike an unclassified literal diagnostic, its rule map must represent six purposes: `LocalPath`, `PrivateEndpoint`, `Secret`, `GeneratedArtifact`, `FixtureClaim` and `InternalIdentifier`. Each entry pairs a private rule identifier with its class and a typed pattern. Missing classes, empty/invalid patterns and exhausted bounds fail before successful evidence is produced. Local preparation requires this policy; it cannot substitute the literal-only diagnostic.

The pure compiler supplies four lexical mechanisms:

- `Literal` searches exact case-sensitive supplied bytes. It does not guess secrets.
- `PathTree` matches an explicitly selected absolute root spelling and its path boundary. It does not resolve files, links, alternate separators or canonical paths.
- `Host` matches an exact ASCII hostname case-insensitively at label boundaries, including its terminal DNS dot. It does not infer a subdomain wildcard or query DNS.
- `IpNetwork` matches native textual IP/socket candidates within an explicit network. IPv4 and IPv6 remain distinct, including IPv4-mapped IPv6. Zone identifiers, browser-specific numeric address forms and dynamically constructed endpoints require other explicit rules or later qualification; this is not URL execution.

Patterns run across filenames, raw bytes and every supported decoded channel. Authored code examples are inspected too: neither a code block nor an apparent example automatically grants an exemption. Matches retain the declared class, rule, file and representation without exposing the matched payload in ordinary error text. Comparison work and complete findings encoding have explicit ceilings. No file is scrubbed, excluded or rewritten to obtain a pass.

The report binds the exact policy encoding, the versioned generic matching/decoder profiles, the output inventory and complete findings separately. These are deterministic evidence identities, not policy authentication or an Engine binary attestation. Diagnostic reports have no policy identity. The instance owner must still supply adequate prohibited values, private networks, internal identifiers and applicable generated-artifact selections. Six populated classes alone do not prove that selection complete or approved.

This check provides mechanical policy coverage and durable preparation binding, not a certificate. The supported-profile limits above, unqualified binary/container formats, owner approval and complete certified input identity remain separate gates.

## Markdown channels

A producer-declared Markdown output, a `.md`/`.markdown` file, or a `text/markdown` data URL uses the selected native Markdown parser. The `gazette-disclosure-markdown/2` profile inspects decoded text, code and labels, link destinations and titles, heading metadata and footnote identifiers. It also joins the parser's text fragments so inline emphasis or code boundaries cannot hide a split literal; structural separators remain intact. Embedded HTML enters the existing HTML, CSS and JavaScript inspection path.

The parser must bind its facts to the exact source and publication-math profile. Unlowered component directives, unavailable parsing, malformed syntax and exhausted bounds fail rather than becoming plain-text success. The scan does not render a Theme, execute code or typeset mathematics. Markdown has independent source, node, depth, fact-byte and output-byte limits; decoded values also consume the whole scan's shared budgets.

Generated Rustdoc `.mdx` companions use the separate `gazette-generated-literal-mdx/1` grammar. It admits only the target title envelope, finite inert markup and JSON string carriers in code or headings. Exact decoded literals enter the HTML inspector as text in their original position, preserving carriage returns and HTML-looking code; the joined text scan also catches a value split between a carrier and surrounding HTML. MathML uses its existing shared invariant. Coverage records the generated grammar, HTML and CSS profiles. Imports, expressions, arbitrary components and general MDX remain unsupported. The decoder reuses Markdown source, node, depth and output ceilings together with HTML and whole-scan budgets; any incomplete grammar or exhausted limit returns no report. Native `.md` companions remain independently inspected.

## Which decoder reads each file {#media-selection-and-json}

Every generated media declaration takes precedence over its configured filename: HTML remains HTML at an extensionless or `.json` route, and the model remains JSON at any selected path. Compiled browser resources select module JavaScript. Undeclared/imported files use case-insensitive suffix candidates, not a claim about their actual serving type. Files ending in `.json`, `.jsonld` or `.map` are additional JSON candidates. A streaming parser visits every decoded key and string value, including duplicate members; it does not collapse objects into last-key-wins maps. Malformed candidates, trailing JSON and the parser's recursion limit return an error, not partial success.

## HTML channels

HTML/HTM suffixes (case-insensitive) use the bounded `gazette-html-token-probe/6;html5ever=0.35.0` profile. Native tokenization decodes HTML text and attribute entities and joins adjacent character fragments across feed boundaries. Title/textarea use their native character-reference context; script/style bodies do not decode HTML entities. Recognized inline classic and module scripts enter the JavaScript scanner under their respective parse goals. Decoded `type` and legacy `language` attributes determine that classification: an explicitly empty type is classic, whereas whitespace-only type is a data block; type overrides language, whose value is not trimmed. JavaScript MIME essence matching does not strip parameters. This follows the bounded [script-preparation classification](https://html.spec.whatwg.org/multipage/scripting.html#prepare-the-script-element), not a simulated browser execution lifecycle.

Inline `application/json`, `application/ld+json`, `importmap` and `speculationrules` bodies enter the same strict JSON scanner, including empty/malformed-body rejection. This is JSON syntax inspection, not validation of those application schemas. Unknown data-script types remain raw text, including empty bodies. A recognized classic/module script with `src` has a separate uninterpreted external-script body channel; other declarations with `src` remain data scripts. Native `on*` attributes also have a distinct, entity-decoded channel, without claiming that the conservative prefix enumerates browser event handlers. The diagnostic scans these values without interpreting their language. No script is executed or fetched. Plain-text declarations and `.txt` files require UTF-8 and receive candidate decoding. Markdown and selected image/font resources use the additional decoders described on this page. Unrecognized formats receive raw scanning only.

All tokenizer errors, including duplicate attributes, return failure. This profile rejects arbitrary foreign content and MathML, unsupported raw/scripting contexts and non-HTML doctypes. One finite inert MathML presentation profile is admitted through the same structural invariant that binds retained KaTeX output to its exact TeX request. Namespace, vocabulary, parent/child, arity, attribute, text and annotation rules fail closed; decoded MathML text and attributes still reach the ordinary disclosure channels.

A finite inert SVG profile inspects the existing navigation paths and folder outlines through these same tokens and limits. It permits a root with direct `path` children and plain-text `title`/`desc` children. Root attributes are `xmlns` (the exact SVG namespace), `viewBox`, `preserveAspectRatio`, `class`, `id`, `role`, `aria-hidden` and `aria-label`; paths permit `d`, `vector-effect`, `class` and `id`, while title/description permit only `class` and `id`. Geometry accepts a bounded lexical subset, with no CSS strings, escapes, functions or URLs; `vector-effect` is `none` or `non-scaling-stroke`. This is token disclosure inspection, not validation of path geometry or runtime changes. Decoded text and attributes reach the ordinary disclosure channels. Other elements, nesting, namespace changes, URL/style/event attributes, comments, CDATA and integration markup inside title/description fail closed. The existing tokenizer still owns entity decoding and malformed-token rejection; no second markup parser is used.

It does not build a DOM, model browser repair, concatenate text across elements, inspect arbitrary embedded attribute languages, or claim that valid tokens make a conforming HTML document. Original raw bytes are always scanned as well. The HTML tokenizer itself does not interpret script/style languages. The application separately decodes CSS style bodies and entity-decoded `style` attributes through the same CSS scanner as external stylesheets. A style element must have no type, an empty type, or `text/css`; another type is unsupported. The HTML coverage record identifies the HTML, CSS and JavaScript decoder profiles.

The `html` limits independently bound each HTML input (default 1 MiB), bytes fed without a token callback (16 KiB), callbacks (100,000), and aggregate decoded text/attribute bytes (2 MiB). The buffering bound is conservative and may include one additional 256-byte chunk; it is not an exact token-length bound. Parser allocation and CPU are not sandboxed: native attribute deduplication can be quadratic within a tag. Consumer observations are speculative until the complete scan succeeds. The application discards them on any later error.

## JavaScript channels

JS/MJS/CJS suffixes are case-insensitive. The `oxc@0.95.0/gazette-syntax-text-v1` profile reuses OXC parsing and semantic early-error checks to inspect decoded identifiers, string literals and individual cooked template chunks. `.mjs` selects module, `.cjs` selects script and `.js` selects the native unambiguous goal. The latter recognizes explicit module syntax; a filename alone does not prove a browser's loading mode. Each file's coverage records this goal. Raw comments and syntax still receive the original byte scan. Native syntax/early errors, invalid UTF-8, lone surrogate values and tagged template chunks without a cooked value reject the complete report.

This does not execute scripts, evaluate or join expressions, resolve imports, decode regex content, type-check TypeScript, accept JSX, or inspect event-handler attribute languages. The parser AST is discarded; it is neither a second publication model nor another bundler. Both external and selected inline scripts use one compiler inspection operation and one application comparison path.

The `javascript` limits apply per file or inline body: 1 MiB original UTF-8 input, 100,000 native AST nodes, depth 128 (Program root counts as one), and 2 MiB delivered decoded text, including repeated values. Input size is checked before native allocation. Node/depth checks follow native parsing and semantic construction, so they do not bound that preceding allocation or recursion and are not a parser-stack, CPU or process sandbox. Later unsupported values or exhausted bounds discard earlier observations; aggregate comparison limits also apply. Coverage remains diagnostic, not qualification for hostile programs.

## CSS channels

CSS suffixes are case-insensitive. The `cssparser@0.33.0/gazette-token-text-v1` profile uses native tokenization to decode quoted strings, unquoted URLs and identifiers, including function/hash names and dimension units, without evaluating expressions or fetching imports. A quoted `url(...)` argument is a string token. Tokens are not concatenated into hypothetical runtime values. Raw comments and syntax remain covered by the original byte scan. Malformed or unsupported token input discards the complete report, even after an earlier canary was found. CSS token validity is not property, stylesheet, browser or disclosure-policy validation.

Original CSS bytes must be UTF-8. One leading BOM is removed after charging the original input size. An optional encoding declaration must be exactly `@charset "UTF-8";` at the start after that BOM; only the encoding label's ASCII case may vary. The declaration still consumes normal token/text budgets. Other, escaped, repeated or misplaced declarations are unsupported, not an instruction to transcode or fall back to a different encoding. HTML style bodies and attributes instead enter as already-decoded text: a leading U+FEFF remains a CSS character, and an encoding declaration is unsupported. The typed input distinction prevents a second byte-decoding pass from dropping meaningful characters while retaining one token-inspection implementation.

The `css` limits apply to each stylesheet, style body or style attribute: 1 MiB original input, 100,000 lexical tokens (including whitespace, comments and closing delimiters), nesting depth 64, and 2 MiB delivered decoded text. An implementation depth ceiling of 256 also applies if a caller requests more. Native token allocation and error cleanup are bounded by input size, not by the callback byte budget; this is not a CPU or allocator sandbox. Missing string, URL, comment or block terminators are rejected even where the native tokenizer would repair EOF. Aggregate probe comparison limits still apply.

## XML channels

XML/RSS/Atom/SVG suffixes (case-insensitive) use the pinned XML profile reported in `gazette_sdk::xml::PROFILE`. A lexical guard rejects DTDs and duplicate raw attributes, including namespace declarations, before the semantic parser runs. The latter supplies XML 1.0 text, adjacent text/CDATA, normalized attributes, explicit namespace URIs (including unused ones), comments and processing instructions to the same scanner. Only explicitly declared namespaces are reported, not the parser's implicit bindings. Names remain raw-file evidence. Comments/PIs do not decode entity-looking strings; CDATA is literal before it joins adjacent text. Text is not joined across elements, comments or PIs.

Original XML bytes must be UTF-8, with at most one leading BOM removed for parsing. A declaration must specify exactly `1.0` and, when present, UTF-8. Multiple roots, malformed/truncated syntax, duplicate expanded attribute names, unbound/reserved namespace misuse and unsupported declarations fail. DTD-looking text inside comments/CDATA is literal, not an external fetch or entity definition. No XInclude, stylesheet, SVG script or other reference is executed/resolved. This decoding does not validate feed/sitemap/SVG schemas or their embedded languages. Selected text/attribute/namespace values can enter the separate encoded-candidate path below. An XML suffix selects a diagnostic parser, not approval.

The `xml` limits default to 1 MiB original input, 100,000 events in each parser pass, depth 128, 128 attributes per element including namespace declarations, 1 KiB names, 64 KiB values and 2 MiB aggregate decoded output. The lexical guard checks depth before semantic parsing. Parser value bounds are not exact allocator ceilings and may overshoot by a token/reference. A native parser limit can be reported with the same private XML rejection class as malformed syntax; neither returns a shortened successful report. Aggregate comparison work still applies.

## Encoded candidates and embedded coverage

One compiler-owned `gazette-encoded/1` decoder handles complete candidate values from paths, plain text, JSON keys/strings, HTML text/ordinary attributes, CSS strings/URL tokens, JavaScript strings/cooked template chunks and XML text/attributes/namespaces. Identifiers, raw script/style bodies and comments are not guessed to be URLs. This is conservative diagnostic interpretation, not proof that the browser uses a string as a URL. Adjacent expressions or separate channels are never joined into hypothetical runtime values.

Data URLs are parsed before generic percent decoding, so escaped separators cannot rewrite their metadata syntax. The native URL parser owns preprocessing and fragment-free serialization. Payload decoding follows the bounded [Fetch data-URL](https://fetch.spec.whatwg.org/#data-urls) and [Infra forgiving-Base64](https://infra.spec.whatwg.org/#forgiving-base64-decode) algorithms. Generic percent decoding preserves `+` and invalid escapes. Bare Base64-looking text is never guessed. The decoder performs no fetches.

Metadata support is deliberately narrower than browser MIME parsing: a valid essence and at most one simple charset value; ambiguous/additional parameters are unsupported. Known text formats require UTF-8; declared UTF-8 or ASCII (with actual ASCII bytes) is supported, not transcoding. Missing data commas, invalid Base64, unsupported selected metadata/charset or malformed selected payload syntax discard the whole report.

Recognized data media select the existing plain-text, JSON, HTML, XML, CSS or JavaScript scanner, not a second parser. Nested candidates share one depth, decoded-value count, aggregate byte budget and literal-comparison budget across the entire tree. Percent decoding alone does not infer HTML or another format. Unknown/binary data payloads receive raw literal inspection and explicit `none` structural coverage; this does not qualify image/container metadata. SVG XML style attributes use the shared CSS decoder; the XML evidence includes its profile. SVG script/style elements and foreign content do not receive embedded-language inspection. Source-map comments, runtime decoding and arbitrary surrounding-text URL extraction remain outside this profile.

Per-candidate input and decoded-output defaults are each 1 MiB. The tree-wide defaults are depth 8 (hard maximum 32), 10,000 decoded occurrences and 16 MiB decoded bytes. Repeated appearances consume budgets separately. Decoded output size is checked before allocating it; URL-parser/metadata scratch is bounded by input size, not the payload ceiling. These are not process/CPU sandboxes. The report records each decoded occurrence's containing path, depth, transformation, exact length/hash, declared data media when present and actual additional syntax coverage, without copying its private payload.

## Raster metadata in policy checks

The SDK selects the raster metadata adapter for its diagnostic probe, required check, typed policy check and connected local preparation. Direct application callers supply that inspector explicitly. Pixel-decoding evidence cannot replace metadata inspection. Without the inspector, a diagnostic reports unsupported coverage; a required check refuses it.

PNG files and `image/png` data URLs share one container/metadata decoder. The closed profile uses pinned PNG and zlib tooling, checks every chunk's CRC, rejects unknown chunks and nonconsecutive image data, and requires exact end-of-image and compressed-member consumption. It visits all tEXt, zTXt and iTXt occurrences, including repeated keywords, language tags, translated keywords and fields after image data. Decoded fields pass through the existing policy matcher and bounded candidate decoder; no metadata is stripped or rewritten. Data URLs carrying a binary charset declaration are refused.

JPEG files and `image/jpeg` data URLs use the same metadata port with a separate closed profile: thumbnail-free JFIF, UTF-8 comments and baseline/progressive Huffman-coded image data, including restart markers. A bounded marker walk checks complete container closure before the existing strict JPEG pixel decoder runs. Every comment reaches the same policy matcher, including repeated or post-scan comments. Invalid UTF-8/control-bearing comments, trailing bytes and unsupported APP containers fail rather than becoming empty metadata success.

EXIF, ICC, XMP, thumbnails, animation and unrecognized containers remain explicitly unsupported. A successful result does not interpret depicted content, steganography or embedded meaning. Original image bytes remain unchanged.

Per-image defaults bound encoded input to 4 MiB, a chunk payload to 1 MiB, logical decompression plus retained UTF-8 fields to 4 MiB, and chunks/fields to 4,096 each. Image-stream inflation needed to verify member closure is charged; separately decoded text and its retained fields are conservatively charged too. JPEG charges its native decoded RGB output plus retained UTF-8 comment bytes; marker payloads and marker count use the existing chunk ceilings. Those bytes and fields also consume the probe's shared decoded-byte/value budgets, including nested and repeated images. These are logical work bounds, not an OS memory sandbox. Any unsupported container, invalid binding or exhausted bound discards the complete report. Successful coverage records the adapter/profile against each exact file or embedded payload without copying metadata into evidence.

WebP uses a closed single-image profile backed by `image-webp 0.2.4`. It accepts simple VP8/VP8L and extended single-image containers with alpha, verifies complete RIFF/chunk lengths, zero padding, order, flags and dimensions, then decodes the pixels under the selected limits. EXIF, XMP, ICC, animation and unknown chunks remain explicit refusals; they are never discarded or treated as empty metadata. The retained encoded bytes remain unchanged. Both `.webp` files and `image/webp` data payloads use this profile. Pixel-buffer work is charged to the existing aggregate decoded-byte budget, so a media-heavy output needs an explicitly sized scan budget; accepting the format does not waive any resource ceiling or image review.

## WOFF2 resources

Producer-declared WOFF2 resources use the selected font inspector to decompress one TrueType- or CFF-flavored font and validate its required `head`, `maxp` and `name` tables. The scanner verifies the exact encoded and reconstructed identities, then checks the complete reconstructed SFNT bytes against the supplied literals. It preserves the original resource. This is byte inspection, not interpretation of glyphs or every metadata encoding.

The current dispatcher selects this profile from the producer's WOFF2 media record. An imported `.woff2` filename or font data URL alone does not select it; inspect the report's actual coverage. Missing inspectors, malformed fonts and exhausted limits cannot become required-check success. Font limits independently bound encoded bytes, reconstructed bytes, table count and glyph count; reconstructed bytes also consume the whole scan's decoded-byte allowance. See [WOFF2 resources](page-resources.md#woff2-fonts) for resource selection.

Coverage remains private mechanical evidence, not owner approval, a certificate or permission to publish.
