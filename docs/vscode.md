# VS Code integration

Gazette meets each actor where it works, with the same publication model behind every mode: a person writing in an editor arrives through the language server, an agent helping in Claude Code or Codex through the MCP server and its skill, and a build or integration through the command line and SDKs.

Build the Gazette extension from source and load it in a separate VS Code development window. Then connect it to the same publication you can already preview. If the extension is installed and you want to edit a publication, start with [Write with feedback](https://rawthoughts.in/gazette/write-with-feedback/).

The extension starts one [language server](lsp.md) for each configured workspace folder. Microsoft's language client handles LSP updates. Gazette's Rust SDK handles configuration and content analysis. When you ask for an application operation, the extension starts a separate MCP process and displays its result.

The extension is named Gazette, with publisher `rtepl` and extension ID `rtepl.gazette-vscode`. These are its manifest identifiers; the checkout instructions below load it locally. Marketplace publication remains a separate release step.

**Gazette: Check Saved Site** checks the active file's workspace using the same native check as `gazette check`. With no active workspace file, it uses the only open folder or asks you to choose among several. The result opens as plain text with file names, line numbers and missing IDs. It checks saved files; save your edits first to include them. The language server continues to report problems in unsaved text.

## Open the local extension from a source checkout

For an installed VSIX, follow [the installation guide](../integrations/vscode/INSTALL.md). To work on this editable checkout, use VS Code's [extension development host](https://code.visualstudio.com/api/working-with-extensions/testing-extension) to load this checkout. You need a built Gazette executable, VS Code 1.96 or newer, Node and npm for the locked extension dependencies, and a local authoring folder. Build the executable using [Installing and running Gazette](install.md).

::::steps

:::step "Build the extension"

From the Gazette repository root, install the extension's locked dependencies without lifecycle scripts and compile its TypeScript:

```sh
npm --prefix integrations/vscode ci --ignore-scripts
npm --prefix integrations/vscode run build
```

:::

:::step "Select your publication"

Open the folder you created in [Your first publication](https://rawthoughts.in/gazette/first-publication/). Keep its files and preview; this setup adds editor feedback to the same writing.

The starter already supplies **.vscode/settings.json**:

```json
{
  "gazette.workspace.configuration": "gazette.toml",
  "gazette.workspace.discovery": true
}
```

Gazette selects pages, included configuration, manual navigation and package manifests from that workspace. A new page beneath a configured content source is admitted when first opened. Sources outside the open folder use the workspace's configured roots and machine binding. For an older starter, remove its explicit `gazette.workspace.documents` list to use [automatic discovery](editor-discovery.md).

:::

:::step "Open a trusted development window"

Launch the extension in a separate window. Substitute your own absolute paths; the profile and extensions directories should be new, short local paths. VS Code uses a socket below the profile on macOS, so a long path can prevent startup. The `code` command is VS Code's [command-line launcher](https://code.visualstudio.com/docs/configure/command-line).

```sh
code --new-window \
  --extensionDevelopmentPath="/absolute/path/to/gazette/integrations/vscode" \
  --user-data-dir="/absolute/short/gazette-editor-profile" \
  --extensions-dir="/absolute/short/gazette-editor-extensions" \
  "/absolute/path/to/gazette-notes"
```

In this separate window, review and grant trust to the authoring folder when VS Code asks. Set **Gazette: Server Path** in **User** settings to your exact absolute Gazette executable. Keep that executable setting out of the workspace JSON above. Then run **Gazette: Restart Server** from the Command Palette.

:::

:::step "Check feedback and navigation"

Open **gazette.toml** and use Go to Definition on `'owner:field-notes'` to select the principal declaration. Change a publisher reference to `'owner:missing'` without saving; Problems should show the unresolved reference. Restore the original value to clear it.

Open **content/keep-the-context/index.md**. Its heading should appear in Outline. **Gazette: Page Details** opens the page's source context. Return to [Correct a declaration in the publication](https://rawthoughts.in/gazette/write-with-feedback/#correct-a-declaration-in-the-publication) to edit the article and see its saved result in your preview.

:::

::::

If the extension does not load, confirm the development path names the folder containing **package.json** and that the build produced **bundle/extension.js**. If the language server does not start, check workspace trust, the User executable setting and the selected root configuration; inspect **Output → Gazette** for the cause. A readable configuration with a typo remains open for correction, including unsaved repair. Unrelated folders without that configuration stay inactive. For client development, use the [extension build and checks](../integrations/vscode/README.md#build-and-check).

## Configure the files the extension reads {#explicit-selection}

An installed platform package uses its included Gazette program when `gazette.server.path` is empty. To choose another program, set that User or machine setting to an absolute path; a source-checkout development host requires this explicit setting. Workspace, folder and workspace-language overrides fail, even in trusted repositories. The extension starts the selected executable with the fixed argument `lsp` and the folder as its working directory. It uses no shell, PATH fallback or extra arguments.

| Folder setting | What it controls |
| --- | --- |
| `gazette.authoring.enabled` | Own an authoring session. Defaults to true. In a multi-root workspace, you can disable it for the whole workspace and enable it on the composition folder only. Source-only folders still supply their named files. Disabling stops the session and releases its files. Application commands remain available. |
| `gazette.workspace.configuration` | Folder-relative workspace configuration. Defaults to `gazette.toml`. |
| `gazette.workspace.discovery` | Discover sources from the configuration when no explicit source lists are supplied. Defaults to true. |
| `gazette.workspace.manualSupport` | Additional manual files outside configured navigation, as `{ source, path }`. Each path is relative to its configured Gazette source. |
| `gazette.workspace.configurationDocuments` | Folder-relative TOML include files. The compiler checks that they match the root's `includes`. |
| `gazette.workspace.roots` | Additional opened folders as `{ id, uri }`. The anchor folder uses `workspace` as its root ID. |
| `gazette.workspace.documents` | Authored Markdown, article-owned `notes.md` or project-documentation files. Give a relative path within the anchor folder or use `{ root, file, path }` to set the physical file and its logical compiler path separately. Notes are editable documents under their article ID. |
| `gazette.workspace.manuals` | Manuals with their configured `source` ID, root, file and logical path. |
| `gazette.workspace.pagePackageInputs` | Opaque files belonging to a page bundle. |
| `gazette.workspace.packageBaselines` | Named `theme` or `plugin` manifests with their package IDs. |

Automatic discovery reads text selected by native workspace roles and retains asset metadata without reading asset payloads. The [discovery contract](editor-discovery.md) explains exact includes, manual support, package manifests and refresh. Nonempty explicit source lists keep the older named-file mode; set `gazette.workspace.discovery` to false to choose that mode even with empty lists. The remaining paragraphs in this section describe its locators and opened-root requirements.

If the server cannot start, a notification names the cause and offers **Choose Gazette Program**, **Open Settings** and **Show Output**. Choosing a program saves its path in User settings. Repeated attempts with the same failure do not stack notifications. If one folder has invalid settings, fix it and run **Gazette: Restart Server**. Other folders can start independently. Gazette's editor language highlighting does not make a file part of the workspace. Source parsing and membership belong to the native compiler and SDK.

Paths keep their literal percent signs and Unicode. Empty or dot components, traversal, backslashes, controls and duplicate physical files across roles fail. Every file must belong to the configured opened root. For nested folders, use the nearest owning folder as VS Code does. A logical compiler prefix supplies no editorial membership or public route.

The extension sends file locators through LSP `native_selection`. It does not read the bodies itself. The request allows 4096 files and 512 KiB for serialized locators, below the default 1 MiB incoming frame. The server reads the named regular files. TOML limits apply to configuration and package manifests, and content limits apply to authored files. One entry/byte budget covers all roots. SDK limits also count IDs, baseline text and open edits. See [the file-backed LSP contract](lsp.md#explicit-native-selection) for initialization.

The files are read during one request, but changes across roots are not atomic. When a watched source changes, [the client requests a fresh read](#sessions-and-overlays). Invalid TOML and unresolved references remain available to diagnose. An editor read does not authorize a build or code execution.

## Visible authoring context

Open the workspace containing `gazette.toml`. Discovery follows its configured source roots. Explicit distributed selections still require their named folders to be opened. Configured TOML modules, package manifests and the `+++` frontmatter in Markdown get TOML highlighting. Highlighting leaves source membership unchanged.

| Command | What you can inspect |
| --- | --- |
| **Gazette: Page Details** | Current field meanings, diagnostics, cursor assistance, inherited identity and page companions in the Gazette Authoring view. Refresh clears outdated results. |
| **Gazette: Inspect Current Editor with MCP** | A read-only MCP query over the current editor snapshot, including unsaved text and versions. Results name Gazette as the provider and link to ranges when the analysis reports them. Export the full private result from the view if needed. |

For automatic discovery, the MCP handoff transfers only unsaved text, up to 12 MiB under a separate 16 MiB frame limit. It retains the complete selection and clean open versions. The receiving SDK reads saved files and checks their baseline hashes before replaying edits; a changed saved file or changed membership refuses the handoff. Saved inputs still have native source and retention limits. Explicit supplied-buffer selections transfer their complete bounded inputs. A change during transfer or querying invalidates the result. The temporary MCP handle closes after the query. Build commands still use the inputs you give them.

## Sessions and overlays

A composition client selects either native discovery from one owning folder or an explicit root and file set. It requires the server advertisement **gazette.authoring = 0.1.0** inside experimental capabilities and standard document-symbol support. Discovery also requires the advertised `gazette/selectSource` method. A server that does not advertise these contracts is stopped; there is no independent-buffer compatibility mode.

In discovery mode, the client asks the server whether a newly opened native path is selected before forwarding its text. The returned selection supplies exact identities and shared-file custody. Unrelated text is not forwarded. Explicit mode uses exact file selectors and opened-root ownership. Configured TOML files receive Gazette language highlighting; source roles remain native workspace facts.

The client sends already-open and newly opened selected buffers, including unsaved text, as overlays. Full-text changes retain editor versions. A save acknowledges the exact open text as the new retained baseline; closing removes the overlay and restores that baseline in the server. Every selected configuration module and content source participates in the same composition's analysis. An open module's current buffer replaces its captured baseline throughout the resolved graph. Diagnostic interpretation, cross-document consequences and symbol construction remain in Rust.

The extension watches exact selected files and, in discovery mode, the declared configuration files and separate machine binding. Closed-file changes refresh the existing native session while preserving open overlays. Explicit mode instead retires the affected composition and captures its fixed selection again. An observed external change to an open text file is deferred until it closes. New content files can be discovered on first open. Snapshots, authoring context and source navigation from pending or replaced clients are refused.

The diagnostic transport guard rejects foreign URIs, stale versions and unversioned baseline results while the source is open, including empty results. It rechecks editor identity/version after asynchronous conversion and before applying results. One active conversion plus one coalesced latest packet per selected source bounds queued work; retired clients cannot apply late results. Version 9.0.1 discards the protocol version before ordinary diagnostic middleware. The narrow bridge therefore registers a raw diagnostic notification handler through the client's **public onNotification API before startup**. The pinned client installs pending public handlers after its builtin diagnostic handler; the registered handler retains the original version through public conversion and collection APIs. There is no private-method override or second diagnostic model. A dependency upgrade must requalify this registration order and packet delivery behavior.

Definition, reference, workspace identity hover and reference-completion requests share a selected-buffer guard around the standard language-client middleware. Before forwarding and after asynchronous transport/conversion, it checks current folder ownership, the exact open-document object, version, source text, cancellation and session retirement. Changed or retired requests return no result; current transport failures remain failures. Rust supplies locations, Markdown hover, schema-derived field/enum assistance and source-bound reference replacement proposals; the extension neither parses labels nor constructs targets, identity descriptions or a completion catalog. Supported contexts and strict-versus-tolerant analysis follow the [shared LSP contract](lsp.md). The editor applies a user-selected proposal to its buffer and sends the changed source through ordinary native reanalysis. This protects the implemented workspace identity queries, document-local heading and footnote navigation, and selected cross-document definition/reference results. Every returned selected sibling is rebound to the exact open document, version and text observed at request time before the result is admitted. General repository-wide navigation, rename and automatic source writes remain outside this contract.

Changing source settings restarts affected compositions. Folder addition and removal invalidate compositions that depend on those roots, including nested ownership changes. Shared physical-file custody prevents two compositions from claiming the same selected file. Restart requests coalesce. Each session retains custody of a client whose stop failed; no replacement is admitted for that folder while cleanup is uncertain. Startup failure attempts cleanup, and shutdown attempts every folder even if one fails. Transport errors do not trigger automatic restart loops. Generic lifecycle logs omit source text, executable paths and transport payloads; native diagnostics remain private editor data.

The authoring path requires trusted local file workspaces. It does not install tools, download binaries, save sources, execute notebooks, build, sign or publish. Selecting an executable is user trust, not release authorization.

## Explicit application operations

The separate [application command controller](../integrations/vscode/src/applicationCommands.ts) uses [one native MCP process](../integrations/vscode/src/applicationSession.ts) per explicitly selected folder. It uses the same bundled program or explicit User override as the language server, with the same trust and workspace-override checks. It starts the fixed **mcp** command with a bounded response-size option, without a shell or arbitrary argument customization. The native result limit stays below the editor's 8 MiB frame limit so complete multi-site receipts retain envelope room. It does not depend on a successful LSP source capture. No configured authoring source is read merely to start this operation path.

The command palette exposes:

- **Gazette: Run Command…**: find any CLI command by its readable name or command spelling, then prepare its request. Choosing a command opens the request without executing it; **Execute Operation Request** runs the reviewed input. For a keybinding, use `gazette.runCommand` with the CLI command spelling as its `args` value, for example `"check"`; this entry also activates Gazette when the extension has not started yet.
- One registered command identifier for every CLI application-command family. The extension-owned [route catalog](../integrations/vscode/src/applicationCommandCatalog.json) names the corresponding MCP operation or operations; it contains no schemas, defaults or implementation. Existing `gazette.application.*` keybindings and API calls also start the extension when needed. These specialist identifiers stay out of the Command Palette. Direct invocation resolves only the family's mapped live tools; all mapped names must exist in the freshly discovered catalog or the command fails before a request is opened.
- **Gazette: Create Operation Request**: choose a folder, discover the live native tool catalog, select one live capability neighborhood, narrow it by the native operation intent, and then select one operation. Intent groups containing common operations precede advanced-only groups, and common operations precede advanced operations within a group, so the complete lifecycle is not exposed as one flat decision surface. The editor validates and retains the native capability, intent, effect, prominence, closure, typed relations and terminal-result owner; it does not reconstruct those facts from names. Then choose a bounded guided request or the explicit advanced JSON editor. The complete raw catalog is not opened implicitly; **Show Application Tools** remains the explicit drill-down. An optional exact tool-name command argument is also checked against that same fresh catalog. This generic picker preserves full CLI-backed MCP coverage when native tools evolve. The guided path derives required and optional fields, descriptions, fixed choices and primitive input affordances from that exact live schema. It begins with no optional choice, never applies a schema default and delegates authoritative validation to native Rust. Advanced JSON retains the complete structural projection with every scalar leaf `null`; there is no handwritten schema inventory, permission default, path/tool substitution, enum choice or schema-generated approval. Guided input is deliberately limited to schemas with at most eight required live-schema leaves. Those schemas use native VS Code Quick Input steps with a visible step/total, Back navigation and cancellation; the controller rechecks the folder-bound session after every step. A schema with more than eight required leaves opens Advanced JSON directly instead of presenting a long serial wizard. Cancelling a guided step creates no request buffer, and neither guided preparation nor automatic Advanced JSON opening dispatches a tool.
- **Gazette: Execute Operation Request**: dispatch the exact associated editor buffer. The explicit command is sufficient only when the live descriptor coherently identifies a read-only, idempotent, non-destructive, closed-world operation. Every local, destructive, external/open-world, repeat-sensitive or conflicting profile requires an additional modal confirmation. The binding contains its URI/document object, original folder, original session and selected tool. Native Rust accepts or rejects the request; the extension does not duplicate operation validation or permission rules. Live annotations are presented as a plain effect profile: read-only versus state-changing, destructive, external/open-world and repeat-sensitive work are visible before dispatch. The editor never infers authority consumption from a state change. Conflicting native hints are shown as an unknown effect rather than guessed.
- **Gazette: Show Application Tools**: show the raw private live catalog without invoking a tool.
- **Gazette: Show Application Result**: reopen the last retained raw JSON-RPC response without invoking the operation again.
- **Gazette: Stop Application Session**: explicitly retire the native process. Failed or uncertain cleanup retains custody and forbids replacement.

Request and result documents are unsaved private JSON text. The parsed catalog is used only for picker presentation; original schema and result bytes remain raw, including integer/string lexemes. The exact argument text is passed to the native transport, which removes only framing whitespace outside strings for LF delivery; it never serializes a parsed argument object. Native duplicate-key, integer and operation-schema checks remain authoritative. After a terminal native response, the editor first shows a concise success, reported-failure or native-RPC-error postcondition with an **Open Full Receipt** action. The exact raw private response is retained and opens only through that action or **Show Application Result**; it is never copied into the notification or rewritten into a successful result. When live metadata names another operation as the terminal-result owner, success instead says that the held lifecycle remains active and names that exact owner. After a successful native result, typed relations and the terminal-result owner may be offered through **Prepare Canonical Follow-up…**. Choosing one first delivers the retained receipt, then opens only an inert schema-derived request for the selected live operation. It never dispatches, retries, infers authority or treats the relation as permission. The editor offers no automatic follow-up effect. No source, request, schema or result is automatically saved. Visible editor errors preserve the transport's safe operational distinction: not-dispatched work can be corrected, native failures require inspection of retained evidence, and an unknown post-dispatch outcome explicitly forbids blind retry and points to inspection/read-back/recovery. Private native text and paths are never copied into the notification.

Before dispatch, the controller rechecks current trust, machine settings, exact folder membership, active document object, version and full text after dialogs. Cancellation before any required confirmation sends no tool call. Saved/copied request buffers cannot acquire a binding by matching content. Folder removal, executable changes and clean session retirement invalidate old bindings; no request silently moves to a replacement process. Application sessions do not restart with LSP settings.

Full native results open as read-only, named JSON documents. Reopening the same result reuses its view; it does not create a dirty untitled buffer or require a save to close. Closing the view does not discard the retained session receipt.

After dispatch, a folder/editor change cannot discard the returned receipt or claim cancellation of native effects. The raw response is retained before the postcondition is shown. Declining its receipt action or a display failure blocks another dispatch until **Show Application Result** delivers the evidence. There are at most 32 retained folder sessions and 32 open prepared requests. The native transport independently bounds each request frame and response; these host bounds do not replace operation-specific native limits or effect permissions. One in-flight native request is allowed per session; concurrent calls are refused, not queued or retried. A busy/uncertain process is never killed to admit another. Completed tool failures, pre-dispatch refusals and unknown outcomes remain three different editor states. An unknown or absent native dispatch disposition for `tools/call` is presented as outcome-unknown, retains process custody and forbids retry. Catalog discovery is effect-free: failed `tools/list` retires that catalog session without claiming that an application operation ran. The message names any exact canonical inspect, observe, reconcile or recover relations as inert evidence or recovery-related guidance only. Their relationship does not make them safe or authorized. A completed failure may prepare one of those requests after displaying its receipt; neither path runs a follow-up effect.

Stopping is graceful EOF plus observed process completion, not cancellation, successful native handle closure or release settlement. Explicitly finish/close native handles using their owning tools before retirement. Extension shutdown attempts both authoring and application cleanup, preserving failed custody. Selecting a tool or a trusted executable does not authorize its effects: supplied native permissions, source pins, signed authority and operation invariants still apply. This editor path introduces no tool installation or publication policy.

## Editor context and export

| Task | VS Code surface | Native result |
| --- | --- | --- |
| Follow a publication or document reference | Open Link, Go to Definition, Peek Definition | Exact selected source and available heading position |
| Find uses or move within a document | Find All References, Outline, Go to Symbol | Current native associations and symbols |
| Understand or correct a field | Hover, completion, Quick Fix | Shared schema and reviewed current-source proposals |
| Inspect page ownership | Gazette Authoring: Page and companions | Owning source, content identity and companion roles |
| Investigate a failure | Problems and Gazette Authoring: Diagnostics | Current failures and actionable companion warnings |
| Share the current editor selection | Inspect Current Editor with MCP, Export Current Editor Context | Explicit retained inputs, hashes, versions and generation |

After a successful `render_sites` operation, **Open Build Input** follows a compiled resource to its exact selected local stylesheet or script. Gazette checks the workspace root and retained digest; a changed file needs a fresh build. Ordinary CSS and JavaScript providers continue to own editing in those files.

Use Open Link (or modifier-click) on a resolved publication wiki link, internal Markdown link or editorial relationship to open its exact selected local source. Go to Definition, Peek Definition, references and symbols use the same current native associations. The Gazette Authoring view also offers Open referenced source at the cursor, plus navigable owning page and companions. Ordinary ownership does not add a Problems entry; actual failures and unhandled companions remain visible.

Export and inspection run one at a time. Progress appears in the status bar through capture and the final write or query, followed by one result message. Use **Cancel Gazette inspection** in the status bar to cancel a running inspection. Cancelling either chooser also releases the operation so another can start.

Export Current Editor Context creates a fresh private handoff. With automatic discovery, `gazette-editor-context/2` contains saved workspace choices and configuration hashes, the complete current manifest, active document/byte offset and only unsaved text. Clean open documents keep their versions without copying their text. Opaque files keep their observed paths and sizes, without reading asset payloads or claiming their contents match. The receiver needs access to the same saved inputs and separately grants permission to read them. A native session using an older server without this handoff receives a clear refusal; it does not fall back to copying every source.

Verify a native export with `gazette import-editor-context --allow-read < editor-context.json`, or pass it to MCP's `authoring_import_editor_context` with outer `allow_native_reads: true`. For a local file carrier, also supply its exact absolute path, independently retained SHA-256 and `file.allow_read: true`. That file permission is separate from workspace-read permission. Inspection from the editor explicitly permits the native reconstruction for its current selection. Neither an exported path nor text inside the export can grant permission.

Explicit supplied-buffer selections continue to use the self-contained `gazette-editor-context/1` format, with complete authoring inputs and captured opaque page inputs. Verify that format with `gazette import-editor-context < editor-context.json`. Both formats use a save dialog requiring a fresh file; existing exports are never overwritten. Their receiving client must admit the complete bounded request rather than omit inputs to fit a smaller frame. The returned MCP session generation identifies the imported state; close the session when done. LSP and MCP generations belong to their own sessions; hashes and overlay versions establish parity.

## What to verify after changing the integration {#proof-boundaries}

Run the [extension checks](../integrations/vscode/README.md#test-a-client-change) when changing the client, then exercise the affected behavior in a VS Code development window. In particular, check unsaved text, navigation and stale-result rejection after changing the language-client dependency. Tests cannot show how a diagnostic or action reads in the editor.

This setup loads source from your checkout. It does not install a Marketplace package or establish that a VSIX works. Test packaging separately if you distribute the extension.
