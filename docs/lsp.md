# Authoring language server

Gazette meets each actor where it works, with the same publication model behind every mode: a person writing in an editor arrives through the language server, an agent helping in Claude Code or Codex through the MCP server and its skill, and a build or integration through the command line and SDKs.

Open the publication folder containing `gazette.toml`. In Claude Code, [load the Gazette plugin](../integrations/agents/README.md#claude-code); it names the local executable and supplies the native discovery settings. In VS Code, use the [editor integration](vscode.md). Neither path requires a generated list of every article. The plugin's actual Claude language-server demonstration is still under qualification; the protocol below describes the implemented server contract.

Start `gazette lsp` to add Gazette diagnostics, navigation and completions to an editor. One process analyses one workspace, even when it contains one site. It uses framed Language Server Protocol over standard input and output, with analysis from the [shared application](../crates/gazette-application/src/authoring.rs) and [Rust SDK](../sdk/rust/src/authoring.rs). The session tracks unsaved edits. It does not build or publish a site.

| If you are… | Start here |
| --- | --- |
| Writing in VS Code | [Write with feedback](https://rawthoughts.in/gazette/write-with-feedback/) |
| Connecting Claude Code | [Gazette agent plugin](../integrations/agents/README.md#claude-code) |
| Building the VS Code extension | [VS Code integration](vscode.md) |
| Implementing another client | [Initialize the session](#explicit-initialization), then [send changes and track versions](#baselines-overlays-and-generations) |
| Adding a feature to an existing client | [Diagnostics and symbols](#diagnostics-and-symbols), [current context](#current-editor-context-and-transport-handoff) or [privacy and limits](#bounds-and-privacy) |

## Explicit initialization

For configuration-driven selection, new-page discovery and repair of invalid native configuration, use [editor source discovery](editor-discovery.md). The fixed selections below remain supported; their shared version, query and transport contracts also apply to discovered sources.

Send initialize with ordinary LSP capabilities and one of two explicit `initializationOptions` shapes. The supplied-buffer form is:

    {
      "workspace": {
        "uri": "untitled:workspace.toml",
        "path": "workspace.toml",
        "text": "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n"
      },
      "workspace_documents": null,
      "documents": [
        {
          "uri": "untitled:article.md",
          "path": "article.md",
          "text": "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Example'\nlifecycle='draft'\n[content.structure]\nkind='page'\nfamily={kind='article'}\n+++\n# Example\n"
        }
      ]
    }

Use `workspace_documents: null` for one supplied workspace buffer. For a modular workspace, put the root in `workspace` and its included TOML files in `workspace_documents`, using the same `{uri,path,text}` form for each. The array must match the root's non-recursive `includes`. Gazette does not discover extra files.

Give each document a role and a logical path. The URI identifies the document for the protocol. Gazette never opens it or infers a role from the URI, suffix or editor language. Duplicate paths or URIs, invalid logical paths, unknown fields and input above the limits reject initialization. Single-buffer and modular initialization are separate choices. A failure does not switch modes.

Supply only the configured text you want to analyse. This session does not capture all of a publication's assets. Plain project Markdown needs its configured workspace binding for semantic checks. Having a heading does not make a file part of the publication.

Check the server capabilities during initialization. The server advertises full-text synchronization, UTF-16 positions, document symbols, workspace-symbol search, identity hover, typed replacement of workspace references, reviewable diagnostic fixes and `experimental.gazette.authoring = "0.1.0"`. It supplies links and navigation for within-workspace TOML, configured Markdown documents, headings and footnotes. The [VS Code client](vscode.md) checks these capabilities. Each folder session is separate, so an editor cannot assume documents in another process can be queried.

### Let the server read named files {#explicit-native-selection}

To have the server read the files, specify the workspace root and every input role. For a single directory, set `rootUri` or use one `workspaceFolders` entry. Send:

```json
{
  "native_selection": {
    "allow_read": true,
    "workspace": "workspace.toml",
    "documents": ["content/article.md"],
    "manuals": [{"source": "source:manual", "path": "docs/index.md"}],
    "page_inputs": ["content/article/diagram.svg"],
    "package_baselines": [
      {"kind": "theme", "package": "theme:base", "path": "themes/base/theme.toml"}
    ]
  }
}
```

Set `allow_read: true` before asking the server to open files. If you use both root fields, they must identify the same absolute `file:` directory. This form supports at most one workspace folder. Gazette loads the root document, its non-recursive includes and the named regular files. It never scans a directory, follows symlinks, guesses roles from suffixes or reads from the current directory. A failed read does not switch to supplied buffers.

For explicitly distributed editor sources, use a rooted selection instead:

```json
{
  "native_selection": {
    "allow_read": true,
    "roots": [
      {"id": "workspace", "uri": "file:///work/publication"},
      {"id": "content", "uri": "file:///work/owner-content"}
    ],
    "workspace": {"root": "workspace", "entry": "gazette.toml", "path": "gazette.toml"},
    "workspace_documents": [
      {"root": "workspace", "entry": "config/sites.toml", "path": "config/sites.toml"}
    ],
    "documents": [
      {"root": "content", "entry": "blog/example/index.md", "path": "authored/owner/blog/example/index.md"}
    ],
    "manuals": [],
    "page_inputs": [],
    "package_baselines": []
  }
}
```

Each root must identify an opened `workspaceFolders` entry. If present, `rootUri` must name the configuration root. Put configuration modules under that root, using their relative entries as logical paths. This editor form opens the named configuration files even if they contain errors so the editor can show them. The compiler then checks the required include list. The single-directory form reads that list through the strict loader.

Other locators give both an `entry` to read and a logical compiler `path`. Neither creates a public address or editorial relationship. Gazette refuses duplicate physical files across roles, unknown roots and conflicting ownership of nested folders. Give each manual its configured `source` key and each package manifest its typed package identity.

Both forms use one combined source/entry budget across roots. Configuration and package manifests use the TOML size ceiling; authored text uses the content ceiling. Opaque page inputs keep their bytes without becoming text buffers. Initialization sends locators within the same frame limit. Reads across roots are not an atomic snapshot.

The SDK builds the session from the files it read. The physical entry and its root determine the file URI. When an editor closes a buffer, the server returns to the original baseline or the latest acknowledged save. It does not read the file again. Permission to read these files does not allow a build or publication.

### Check Theme templates through the CLI or MCP {#tera-template-boundary}

Gazette 0.1 uses Theme manifests for component completion and hover. It does not serve the Tera templates named by those manifests as LSP documents. Use the CLI or MCP [template compiler](mcp.md#composition-phases) to check the complete Theme and its virtual template namespace. LSP does not scan template directories or infer roles from filenames, suffixes or language IDs.

## Current editor context and transport handoff

`gazette/authoringContext` accepts `{generation, textDocument: {uri}, position}` with a standard UTF-16 position. It returns current compiler-owned fields and schema facts, native diagnostics, cursor assistance, identity/composition and page-package context. Source digest and open version bind the result. Optional semantic views explicitly distinguish available, inapplicable and unavailable current evidence. The tolerant field outline never replaces strict compilation. Schema errors in a configuration document retain that document's own path and range throughout the compiler, SDK and LSP; they cannot acquire a sibling's span. Semantic failure still refuses compilation. Unambiguous typed source facts remain available for repair: a uniquely occurring unresolved reference keeps its exact owner and can receive reviewed replacement proposals across modules. Repeated or ambiguous occurrences do not acquire an invented source location.

`gazette/editorSnapshot` returns a complete manifest of the current retained selection: generation, roles, logical paths, current and saved-baseline digests, byte counts and open versions. `baseline_sha256` identifies saved text; a non-null version alone does not mean the text is unsaved. `gazette/editorSource` accepts `{generation, document, offset, length}` and returns at most 65,536 bytes as base64 with the same generation, document and offset. Offsets are byte coordinates and may divide a UTF-8 character; decode text only after reassembly and complete digest verification. A generation change rejects subsequent chunks. Opaque companions preserve their byte role.

These requests perform no native read or write. For native workspace discovery, the snapshot also includes `native_selection`: the workspace directory, configuration path, explicit manual selections and hashes of the saved configuration and machine binding. The `editorSnapshot.nativeContextFormat` capability advertises `gazette-editor-context/2`. A client transfers text only for sources whose current and baseline digests differ, retaining the complete manifest, clean open versions and opaque file observations. Export and MCP inspection use the same SDK reconstruction owner. The receiving caller must separately permit native reads; the exported selection grants no permission. Reconstruction checks saved baselines, replays unsaved configuration before content, and refuses changed membership, roles, hashes or observations. File-size observations do not establish asset contents, and reads across files are not atomic.

Explicit supplied-buffer selections retain the self-contained `gazette-editor-context/1` format, including captured opaque inputs. A client can also reproduce those supplied inputs through `authoring_start` and replay open versions through `authoring_open_document`. Native discovery refuses an older server without the compact descriptor instead of copying the entire saved corpus. LSP and MCP remain separate sessions over one SDK analysis owner. A manifest is not a build, publication or permission receipt.

## Baselines, overlays and generations

The [SDK session](../sdk/rust/src/authoring/session.rs) holds the source baseline, full-text edits, versions and generation. LSP checks URIs and request shapes and converts locations to UTF-16. MCP uses the same SDK with UTF-8 byte offsets. Its handle limit and expiry apply to MCP sessions only.

| Editor event | What Gazette does |
| --- | --- |
| `initialized` | Analyse the captured baseline and publish diagnostics. |
| `didOpen` | Start an overlay for a URI in the configured session. |
| `didChange` | Accept one complete replacement with a strictly increasing integer version. Consecutive changes for the same URI may be combined before analysis. |
| `didSave` | Compare the supplied saved text with the open overlay. If it matches, make it the baseline and advance the generation when the baseline changed. It reads no file. |
| Close | Remove the overlay and analyse the original or acknowledged saved baseline. To see external file changes, restart with a fresh read. |

The server logs and ignores duplicate opens, unopened or out-of-session changes, malformed notifications and incremental edits. A full replacement above the session's byte limit is also ignored. None advances the version or generation or stops the server from handling another valid request.

An accepted edit reruns analysis across the whole configured session. For example, an open included TOML file supplies its current declarations, and editing a collection can produce a diagnostic on an unchanged article. The diagnostic uses the article's version and digest with the new generation. An invalid current parse replaces the earlier successful result for diagnostics, navigation and validation. Completion can separately use the preceding workspace parse as described below.

The reader can accept frames while one worker runs SDK analysis. The worker keeps the latest pending change. Requests and open/close events preserve their order relative to changes. If a newer contiguous edit is already queued, Gazette discards the older result. A computation already under way can finish, but it cannot publish an obsolete or partial result.

| Scheduler setting | Limit |
| --- | --- |
| Input channel and backlog | 64 messages each |
| Change notifications in one analysis batch | 64 |
| First batching delay | 4 ms |
| Later batching delay | A quarter of the most recent analysis duration, between 2 and 40 ms |

Batching controls how long the worker waits for changes before starting. It does not bound the total editor response time. The scheduler test queues 64 changes and checks that diagnostics reach the latest version without one update per keystroke. It does not use a machine-specific timing threshold.

## Diagnostics and symbols

Native errors remain application-owned. The SDK's borrowed projection retains codes, logical fields/endpoints, all matching declaration origins and exact path/digest binding. The LSP adds only editor ranges, versions and generation. The same retained Markdown analysis reports source-only renderability failures for raw HTML before Theme rendering. Nested footnotes and references inside image labels use the same emitted-footnote closure as rendering. Backlink counts describe emitted references rather than unused source occurrences. This diagnostic preview neither selects a Theme/component renderer nor changes publication admission or build validity.

- Checked UTF-8 spans become UTF-16 ranges, including CRLF and non-BMP characters.
- data.location preserves the native full-file span and its precision. A construct range is not an exact token or permission to edit it.
- Missing locations use a required LSP document range while retaining a null source location. Logical fields are not converted to guessed field spans.
- Selection-wide failures are explicitly marked; repeating one on selected documents does not blame those files individually.
- Complete-result exhaustion fails explicitly rather than dropping findings.
- A final gazette/analysisPublished notification carries generation and structural validity after that generation's diagnostic notifications. Sequential stream delivery is not atomic client application of the set. An interrupted stream must not be treated as a completed generation.

textDocument/documentSymbol uses shared `workspace_symbols` for the selected workspace and `heading_symbols` for Markdown. Workspace declarations are flat Object symbols named by their native identity, with the original TOML scalar construct as both range and selectionRange. Markdown headings retain the shared allocator's body coordinates checked against exact full source. Neither role invents whole-section ranges or rename tokens. Invalid current syntax or ambiguous declarations refuse workspace symbols; an unrelated corpus syntax failure does not erase a valid workspace index. Corpus failure cannot return previous headings. Structural help does not imply a valid publication.

### Find a symbol in the workspace {#selected-workspace-symbol-search}

`workspace/symbol` uses the SDK's single selected-source symbol search. Its required `query` is a string; empty selects all symbols. Matching uses Unicode lowercase substring comparison, not identity normalization, fuzzy ranking or linguistic case folding. Results are complete current workspace declarations and Markdown headings, ordered by logical path and native construct. There is no disk search.

The server advertises `workspaceSymbolProvider: true` and returns standard `SymbolInformation`: Object declarations and Namespace headings with each actual selected URI and checked UTF-16 range. Native generation, source digest, path and overlay version are checked before projection. The ranges describe constructs, not rename tokens. A changed overlay changes the result; close restores the exact selected baseline. Invalid current selected syntax or exhausted result bounds refuses the whole search instead of returning stale or partial matches. Lazy resolution and partial-work delivery are not advertised. Use the [VS Code source setup](vscode.md#open-the-local-extension-from-a-source-checkout) to exercise symbol search in the editor.

### Workspace declaration and reference navigation

The existing definition/reference methods also consume the compiler's retained workspace association index. The selected workspace's exact UTF-8 source identity is checked before LSP converts its native construct ranges to UTF-16. Only typed workspace references recognized by that compiler owner participate; equal strings in names, comments or other unrelated fields are not references.

In supplied-buffer mode, declarations and references belong to that one workspace buffer. In modular mode, navigation spans the root and its exact admitted set of included TOML documents: a reference in an include can lead to a declaration in the root or another selected include. The compiler identifies each definition and reference's actual document, source digest and span; LSP maps that document to its selected URI and converts positions in the matching buffer. It does not discover unselected modules, read native files during navigation or guess targets from equal strings. Markdown-link, footnote and field-origin queries retain their separate scopes described below.

Invalid current workspace syntax or semantics refuses navigation instead of reusing an earlier index. Valid workspace navigation remains available during an independently invalid selected Markdown corpus. Accepted overlays and close use the same shared Session lifecycle as Markdown navigation.

### Current workspace field origins

The discoverable custom request `gazette/origin` accepts `{generation, query}`. Use the generation from `gazette/analysisPublished`; stale generations are refused. `query` is either `{kind:"field", query:<FieldQuery>}` or `{kind:"composition", query:<CompositionOriginQuery>}`, reusing the native compiler query contracts. Native key/index paths remain string/integer arrays.

The response is `{native, range}`. `native` binds the selected document, logical path, generation and nullable overlay version to the compiler's exact source digest, UTF-8 byte span and (for composition) winning declaration layer. `range` only projects that span to checked LSP UTF-16 positions in the same current buffer. No field-location or inheritance resolver exists in the server. The capability is advertised at `experimental.gazette.origin`.

Origin queries reparse the bounded current workspace using the existing native parser/resolver. They perform no source I/O or publication effects and never substitute a previous successful result. Invalid current workspace, missing identity, absent explicit field, stale generation and query/ result exhaustion are distinct refusals. Unrelated Markdown failure does not hide a valid workspace origin. Closing an overlay restores only its retained baseline. `authoring_origin` supplies the same native result through MCP.

When changing origin queries, run the SDK origin tests and the protocol suites:

```sh
cargo test --locked -p gazette-sdk --test authoring_origins
cargo test --locked -p gazette-lsp --test protocol origin
cargo test --locked -p gazette-mcp --lib authoring::origin
```

### Workspace identity hover

`textDocument/hover` uses the same current native association as definition and reference navigation. A declaration or recognized reference returns `Resolved <native kind>: <identity>` with its original occurrence range. The server chooses the first supported format in the client's `textDocument.hover.contentFormat` preference list. An absent or empty list uses plaintext; Markdown is used only when offered. Opaque identities, paths and values remain literal in either format, and Markdown escaping counts toward the complete response limit. The application's `workspace_identity` result retains source identity, occurrence precision and the original declaration symbol; LSP only presents these facts. A field can also show its description, whether it is required, the values it accepts and any named choices from the shared schema. Unrecognized text returns no hover. Help describes the current configuration; it does not establish a valid publication.

Page-package authoring remains compiler-owned. Within an authored page's `resources[].inputs[]` and `resources[].build.entry` values, completion proposes only paths retained by that page's resolved resource declarations and includes their proven output routes in completion data. Standard definition navigation follows an exact declared input or entry to its explicitly selected retained companion URI; the zero position means the opaque companion has no fabricated editable source span. The generation-bound `gazette/pagePackage` request accepts `{ "generation", "document" }`, where `document` is a selected document or opaque page-input URI, and returns the retained owner/source/companion URIs plus compiler-validated resource programs. Unhandled companions never receive an invented compiled destination.

Resolved documentation builds are target-preflighted without acquisition or effects. Workspace diagnostics retain ordered manual navigation, configured routes, exact source identities/selections, and generated references as distinct items. `documentation_validation_incomplete` is informational and means final route/anchor closure belongs to target assembly but the editor session lacks the complete retained manual/reference trees. Documentation field provenance remains available through `gazette/origin` using the native brand field path; the LSP does not parse Mintlify configuration or implement a second anchor validator.

For a site declaration or typed site reference, the same native identity result also borrows the compiler's effective composition. Hover appends a bounded JSON view, fenced as code only for Markdown clients, showing its Theme, palette and capability selections, including each winning workspace/site layer. Disabled capabilities stay visible; a site Theme replacement does not acquire the default Theme's omitted palette. Neither LSP nor MCP recomputes inheritance or maintains another configuration catalog. These are desired selections, not verified provider availability or build readiness. Origins remain logical declarations, not invented edit ranges.

Label length is checked before formatting, and the complete result is bounded before delivery. Invalid current workspace analysis refuses the query, never substituting an older identity. Accepted edits and close follow the same selected Session lifecycle; unrelated invalid Markdown does not remove valid workspace help. MCP exposes this exact native fact through `authoring_hover`, including precision that standard LSP ranges cannot express and the same site composition object.

### Typed reference replacement and repair

`textDocument/completion` projects the shared native reference queries. For workspace references, `complete_reference` includes unresolved keys. The compiler verifies the native TOML string scalar and its decoded identity; the field's declared namespace and current unambiguous declarations supply the complete candidate set in source order. Each candidate reuses its declaration's original valid quoted literal, including escapes, rather than introducing another TOML encoder. Names, declarations, arbitrary strings and comments return null.

The edit replaces the entire scalar, not its decoded interior. Each item retains the exact preimage, source digest, path, editor version and session generation in `data`; LSP alone converts native byte ranges to UTF-16. The shared response and complete protocol result have independent byte bounds; exhaustion returns no partial list. Invalid syntax and ambiguous declarations offer no repair subset. Unresolved references retain current diagnostics while permitting proposals from the same typed parse. These current-source queries never substitute an older model or admit a partial workspace; the separate interrupted-reference fallback below does not change their contract. The shared result carries the field's namespace and a nullable current target. These are typed proposals, not proof that replacement satisfies every cross-field constraint. Applying clients must keep the current preimage/version and validate the changed workspace. Gazette performs no source writes. The VS Code bridge rejects in-flight results after buffer, version, ownership or session changes. MCP exposes the same result through `authoring_complete_reference` with its exact generation guard. Standard LSP completion requires a single-line replacement range, so multiline reference scalars are explicitly refused by LSP while their native exact-span proposals remain available through SDK/MCP. Replacement text requests `asIs` insertion rather than automatic reindentation. Schema-derived missing-key and closed-literal completion is separately available for workspace TOML and explicit authored TOML frontmatter. The compiler recovers current syntax against the actual Rust/Serde declaration schema; LSP owns only UTF-16 projection and exact edits. Plain Markdown, body positions and open-ended string vocabularies do not acquire invented candidates. General free-text prefix completion remains unsupported. Structured `*.landing.yaml` sources participate in selected-set parsing and resolved cross-document references. Original `component` and direct `arguments` scalars can show the winning Theme template and argument schema. Exact tokens can receive component names, unused schema keys and closed literal proposals. Normalized Markdown body text has no exact inline edit coordinates. Escaped and folded YAML may expose a containing construct for inspection without an edit.

Authored reference help also covers editorial collection, journey and member scalars, authored site and attribution-principal scalars, and non-containing related-content references. Editorial candidates respect the field's target kind and the declaring site's identity. Current declaration literals and source locations supply replacements; the graph still validates residence and cycles after an edit. Related-content candidates include resolved site-specific legal outputs, not their owner-held source identity. If that derived namespace cannot be established, completion refuses rather than presenting an incomplete set.

AST-recognized `[[site:...|label]]` links have destination-only completion, including incomplete or unresolved selectors. Candidates come from selected publication pages and their proved headings and article-owned notes anchors. Code, image labels and unrelated same-word text acquire no symbolic edit. Exact source preimages, candidate definition evidence and complete-result bounds remain shared with SDK/MCP; LSP only converts the range to UTF-16.

Selected `notes.md` belongs in editable `documents`, not opaque `page_inputs`. It retains its article's identity and requires no independent page frontmatter. Exactly selected legal-source bindings receive the reduced native legal schema, reported as `legal_source_frontmatter`, rather than invented site or page fields. Sound scalar assistance can survive incomplete neighboring TOML; this does not make the document valid for publication. Symbolic candidates are publication targets, not evidence of Mintlify manual-link compatibility.

### Completing an interrupted workspace reference

While a workspace reference is being edited, completion can use its last parsed scalar if the current parser cannot supply candidates. For example, deleting the closing quote in `publisher='owner:one'` still lets the editor suggest the declared principals. The SDK keeps at most one previous workspace analysis and the edited source, within the existing retained-source byte limit. Current diagnostics continue to show the syntax error.

This fallback requires every byte outside that known scalar to remain unchanged. A declaration or field change makes the fallback unavailable. Editing another document, saving, closing, changed native membership or unavailable native sources discards the retained parse. Native discovery can retain it only when the selected roles and current inputs are identical. The edit range, source digest, preimage and editor version describe the current buffer; candidate definition positions from the earlier parse are omitted. Results identify their prior source and generation and set `isIncomplete` so the editor asks again as typing continues. Multiline LSP replacements and exhausted response bounds still fail. Quick fixes, hover, navigation, validation and stateless MCP queries never use this fallback.

### Reviewable diagnostic repairs

`textDocument/codeAction` advertises only `quickfix`. It offers the same exact source-bound replacement proposals as completion when the requested range has a byte-for-byte current Gazette diagnostic. The server recomputes that diagnostic from the selected session; stale, altered, non-Gazette or unassisted diagnostics produce no action. Each returned edit names the current editor version when one is open and retains the session generation, source digest and preimage in its proposal data.

Actions are deliberately titled as review proposals, are never preferred and contain no command. Gazette does not apply them, save files, run a formatter or claim the replacement satisfies every cross-field constraint. The client shows the versioned edit for review; accepted text must return through ordinary `didChange` analysis before diagnostics can clear. There is no fix-all action and no second TOML or Markdown parser in the language server.

### Heading and footnote source navigation

Heading links use the same allocated anchor index and strict fragment decoder as publication compilation. A recognized navigational link with no path, such as `#section` or `?view=local#section`, resolves to its emitted heading. Querying that heading returns the complete recognized local reference set. Duplicate automatic slugs and explicit IDs therefore agree with the rendered page; the editor does not slugify text or reverse-map generated HTML.

Images, external links and root-relative links do not become heading associations. Relative document paths use the selected-document association described below, not a same-document fallback. Valid footnote or continuation DOM anchors do not masquerade as heading declarations. Native source occurrences omitted from rendered output remain source-navigation evidence. A malformed or missing local fragment refuses the document's heading-reference result rather than returning a partial list. Structural heading symbols remain independently available; this query is not full publication-link validation.

The shared `source_navigation` operation compares heading/link and footnote occurrences by their native containing ranges. A footnote reference in a heading selects the note; a fragment link inside a footnote selects the heading. Equal ranges with different targets fail as ambiguous. An independently valid footnote association remains available when heading-link evidence cannot be produced.

`textDocument/definition` and `textDocument/references` resolve recognized Markdown footnote occurrences through the shared authoring SDK. The compiler exposes the existing native definition/reference facts; the application retains their bounded document-local associations. Neither the LSP nor VS Code parses labels, invents case-folding rules, reverse-maps HTML IDs or joins equal labels across files. This includes source occurrences omitted from rendered output.

Queries require the exact current selected source and checked UTF-16 positions. The application binds its answer to the logical path and whole-source digest, and lifts body coordinates through the original frontmatter boundary. If a reference occurs within a definition, the smallest containing native occurrence wins. More than one matching native definition fails as ambiguous; undefined footnote-like ordinary text does not acquire an invented target. Unused native definitions remain queryable even when they have no references.

Definition returns one standard location or null; references returns the complete logical-path/source-ordered set and honors `includeDeclaration`. These are native construct ranges, not exact label-edit or rename ranges. The SDK preserves precision; standard LSP locations carry the range only. They authorize no source changes.

Accepted overlays rerun these same facts. Failed current corpus syntax refuses Markdown navigation rather than returning an earlier map; workspace or content-policy errors may still permit explicitly structural source help. Invalid positions, surrogate interiors, unselected URIs and malformed request shapes are rejected. The source projection is bounded across the whole selection before copying; URI amplification and complete response encoding are separately bounded by the server. No partial reference list is returned on exhaustion.

### Follow links between configured documents {#selected-document-markdown-links}

Explicit relative Markdown links resolve only to exact logical paths in the selected source set. The compiler shares publication path decoding and anchor allocation: encoded filenames/fragments and confined `.`/`..` segments work, without extension guesses, directory-index lookup, disk reads or URI inference. A fragment selects its existing heading; a fragmentless link selects the target's whole Markdown body, excluding frontmatter. Valid nonheading DOM anchors do not invent heading definitions. Images, external/root-relative links and unselected targets do not create associations.

Each definition and reference retains its own selected path, complete source digest and native location. Reverse references include the complete selected incoming set plus local heading links. Footnotes remain document-local. Invalid selected target fragments or malformed/escaping relative paths refuse complete cross-document evidence instead of returning a partial or older target; an independently valid footnote can still resolve. Target overlays immediately rebind unchanged incoming links, and close restores the captured baseline.

The Rust SDK navigation tests cover selected-document source/anchor joins, exclusions, overlays, stale-source refusal, failed targets and complete-result bounds. After changing client presentation, test [source navigation in VS Code](vscode.md#editor-context-and-export).

### Open a source link {#open-selected-source-links}

Resolved publication and document references also project through standard `textDocument/documentLink`. The compiler's existing source associations own the link range, target source and heading coordinates. The LSP maps that exact selected target to its client URI; it does not interpret a site URL as a filesystem path. Link data retains the current generation and both source identities. Failed current resolution refuses the result rather than returning a successful stale subset. Normal page ownership belongs to `gazette/authoringContext` and `gazette/pagePackage`, not the Problems list. Unhandled companions still produce their actionable warning.

### Inline heading-link completion

Standard completion also projects `Analysis::complete_heading` for a selected Markdown document. The candidates are the exact emitted headings and URL-ready fragments used by publication rendering, including duplicate-heading suffixes and explicit IDs. They are not an editor-maintained slug catalog. An unresolved fragment can be repaired from current heading facts without borrowing an older successful link-resolution result.

The initial edit profile is a provider-recognized emitted inline link whose literal suffix is `](#fragment)`, including an empty `#`. The decoded destination must match those exact source bytes. Only that destination is replaced; labels, framing, line endings and other content are preserved. Images, external links, reference-style links, titles and escaped/entity destination spellings receive no inferred edit. This is not incomplete-Markdown parsing or cross-document completion. Native containing ranges alone never justify token replacement.

The proposal binds the complete current source digest and exact preimage. LSP adds the session generation, overlay version and checked UTF-16 edit range; MCP exposes the same native proposal through `authoring_complete_heading`. Complete native and transport bounds refuse instead of truncating the candidate set. The existing VS Code completion guard protects current source/version and retired sessions. Selecting a suggestion edits an editor buffer only; ordinary native reanalysis follows, without file writes or publication permission.

### Theme component authoring

Standard completion and hover also project the shared SDK's selected-Theme component facts. Supplied initialization selects typed `theme` or `plugin` package identity alongside its unique file identity, logical path and exact manifest text. The workspace remains the sole owner of its source/manifest binding; native selection supplies exact permitted locators instead of text. Gazette never infers a Theme from `presentation.toml`, scans a folder or opens a configured repository path. Two source bindings may therefore use the same manifest filename without losing their distinct identity.

The application joins an authored document to its configured content source, site, effective composition, Theme source and Theme identity once. The compiler's retained directive header lexer and `Components` validator remain the sole name/argument owners. Completion replaces either the exact component-name slot with the complete selected-Theme catalog, or an exact property-name/closed-literal slot inside that component's TOML argument table with candidates from its native Draft 2020-12 schema. The LSP projects checked UTF-8 spans to UTF-16 edits, including non-BMP text before the slot. Hover shows the exact component name and selected template, with its argument contract as JSON, fenced as code only for Markdown clients. For structured YAML landings, hover also explains that child sections may use components from the effective Theme or ordinary groups with a heading, nonempty body or child section. Completion on an exact child component name shows the selected catalog. Markdown normalized from YAML body scalars does not supply editable ranges. Manifest or document overlays advance the same authoring generation, so changed or closed buffers cannot reuse older component facts.

MCP exposes the same UTF-8 source-bound result through `authoring_component_assistance`; LSP alone converts its exact byte span to UTF-16. The VS Code extension selects only explicitly configured native files and forwards these standard completion/hover results unchanged. It contains no component parser, argument-schema interpreter or catalog.

No general free-text prefix completion, formatting, fix-all action, workspace-folder mutation or notebook synchronization is advertised. The bounded quick-fix action above is the only code-action profile. Unsupported requests receive method-not-found; unknown extension notifications have no effect. Shutdown followed by exit is required for clean termination.

## Bounds and privacy

Defaults: 8 KiB headers; 1 MiB incoming JSON; 2 MiB outgoing JSON and complete diagnostic set; 256 total diagnostics. Selected-source defaults come from the SDK session: 4,096 documents including the workspace, 1 MiB total URI identity bytes and 16 MiB retained baseline-plus-overlay bytes plus the optional prior completion source, matching the default complete corpus budget and including opaque authoring inputs. Identifier, logical-path and text bounds precede selection retention; a refused edit cannot partially mutate a session. Shared workspace parsing defaults remain 64 KiB source, 10,000 nodes and depth 64; content and heading limits come from authoring::Limits. Source navigation defaults to 16,384 retained workspace declaration/reference occurrences, footnote occurrences, heading links, exact inline completion-slot records and selected-document references, and 4 MiB of native association keys/strings across the complete selected set. Query results use only these bounded retained facts.

Framing checks length before body allocation. The duplicate-safe SDK JSON decoder precedes native protocol types. Projection budgets precede retained diagnostic rows; the complete encoded set and each outgoing frame are separately bounded. Native stream failure may interrupt a write and is never clean success. These are bounded operations under the [shared execution model](architecture.md#trusted-code-and-saved-results), not a process sandbox.

Source text, URIs, identities and diagnostics may be private. They travel only over the selected editor process connection. No telemetry or provider access is performed.

To check protocol changes, run the LSP protocol suite, CLI integration suite and shared-session suite:

```sh
cargo test --locked -p gazette-lsp --test protocol
cargo test --locked -p gazette-launcher --test lsp_cli
cargo test --locked -p gazette-sdk --test authoring_session
```

These cover framing, process startup, editor overlays and failed analysis. Continue with [VS Code setup](vscode.md#open-the-local-extension-from-a-source-checkout) to inspect the result in an editor.

## Current editor contract

The sections above define the versioned selected-source contract. The LSP and VS Code client project the shared compiler/SDK facts; they do not implement a second publication model. A current failure cannot borrow an older successful analysis, and an edit proposal still requires an exact current source preimage.

Rename, formatting, fix-all, automatic file writes, notebook synchronization and native build/release effects are outside this protocol. Synchronous SDK analysis already running cannot be interrupted. Generated-source ranges are exact only where the SDK retains an exact origin. For the client implementation and separate editor-host checks, continue with [VS Code integration](vscode.md) and [the client checks](../integrations/vscode/README.md#test-a-client-change).
