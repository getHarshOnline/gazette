# Discover editor sources

Open the folder containing `gazette.toml` to get editor feedback for its configured publication. Gazette reads the root, its exact includes and the separate machine binding, then selects authoring files from the resolved workspace. Sources can live outside the open folder. The configuration supplies their owners and logical paths; opening a neighbouring repository does not add it to the publication.

The VS Code extension uses discovery by default when no explicit source lists are supplied. A folder without the selected configuration stays inactive. The first-publication starter enables this mode, so adding a page beneath its configured content source requires no editor-list update. Existing explicit selections keep their [named-file contract](lsp.md#explicit-native-selection).

## What enters the editor session

| Configured role | Selection |
| --- | --- |
| Workspace | The root and its exact non-recursive includes. The saved machine binding supplies native roots separately. |
| Publication content | Files under each selected content source, with its literal includes, exclusions, subtree and logical placement. Markdown, structured landings and notebooks supply text. Other files supply path and observed size only. |
| Retained-only content | Metadata only, including files whose suffix would otherwise select a text parser. |
| Legal text | Each exact configured legal entry. Siblings are not selected. |
| Authored manuals | Exact files named by manual navigation, plus explicitly retained support files. Generated references are separate. |
| Themes and plugins | Each exact declared package manifest. Discovery does not walk the package's template or dependency directories. |

The shared compiler owns the filename rules and logical placements. Package manifests have package-qualified editor paths, so equal filenames in different packages remain distinct; their native URIs and configured manifest paths stay intact. The filesystem adapter owns confined reads and refuses links below selected roots. Equal repeated bindings collapse only when their native file, logical path and role agree. Conflicting meanings are refused. Discovery describes working files; it neither verifies repository pins nor produces captured build evidence. Asset size is an observation, not a digest or retained payload.

For manual support outside navigation, set `gazette.workspace.manualSupport` to `{ "source": "source:manual", "path": "docs/support.md" }` entries. Each path is relative to that configured source. When migrating an older explicit selection, verify that every retained manual still identifies the same native file before removing its old locator. Nonempty legacy source lists continue to select the explicit mode.

## Initialize another client

Choose one absolute native workspace directory through `rootUri` or a single `workspaceFolders` entry. If both are present, they must agree. Send these initialization options:

```json
{
  "native_selection": {
    "allow_read": true,
    "discover": true,
    "workspace": "gazette.toml",
    "manuals": [
      {"source": "source:manual", "path": "docs/support.md"}
    ]
  }
}
```

Omit `manuals` when configured navigation supplies all manual inputs. `allow_read` permits the native reads for this session; it grants no build or publication effect. The server advertises `experimental.gazette.sourceDiscovery.selectSource = "gazette/selectSource"`.

Before forwarding a newly opened file's text, call `gazette/selectSource` with `{ "uri": "file:///work/content/new.md" }`. It returns `selected`, the current `generation`, and exact configuration `watchFiles`, including the separate machine binding and declared configuration files that are currently missing. An unrelated path causes no source scan. A new path under a configured content source refreshes membership before admission. The server also handles first-open discovery directly through `didOpen`.

Use `gazette/editorSnapshot` for the current selected roles and opaque asset observations. Forward text only for admitted sources. `workspace/didChangeWatchedFiles` refreshes native membership; clients should watch the current selected files and configuration paths. Neither these requests nor the editor writes source files.

## Repair without losing unsaved work

An unreadable root refuses startup. A readable root with a configuration error remains editable. Current root and included-document overlays drive discovery, so an unsaved correction can restore the selection. The machine binding uses its saved contents and remains outside workspace compiler documents.

Refresh preserves open text and versions for files that remain selected and advances the same session's generation. Invalid configuration retains the existing buffers for repair. A native discovery failure makes successful semantic analysis unavailable and reports the reason on the root document; it does not reuse an older success. Correcting the configuration or refreshing restored sources rebuilds the selection. A valid change that removes a file also clears its diagnostics. The client retains unsaved text for files outside the new selection and sends it again if they become selected later.

Closing a configuration overlay restores its native baseline and refreshes membership. Saving acknowledges the exact current text before refreshing. Content edits use the current selected session without walking native sources on every keystroke. All retained text, identifiers, overlays, metadata and diagnostic responses remain bounded by the SDK and transport limits.

The VS Code client waits for native admission and current open text before querying. Configuration overlays are reconciled before content overlays, and a changed native context renews the symbol provider through the pinned client's public feature registration. This refreshes an article's outline after configuration repair even when the article's editor version has not changed.

For the implementation, start with the [native SDK session](../sdk/rust/src/authoring/session/native.rs), [LSP dispatch](../interfaces/lsp/src/documents/state.rs) and [VS Code lifecycle](../integrations/vscode/src/workspaceSession.ts). The [explicit session contract](lsp.md#baselines-overlays-and-generations) continues to govern full-text versions, source-bound queries and reviewed edits.

The [native editor host check](../integrations/vscode/tests/nativeHost.cjs) runs in a real local macOS VS Code with isolated user-data and extension directories. Set `GAZETTE_VSCODE_ISOLATED=1` and `GAZETTE_TEST_EXECUTABLE` to the exact candidate executable. Open the generated first-publication example in a disposable folder named `native-editor-starter`; the check creates a page there and exercises its first-open diagnostic and completion, configuration repair and preservation of an unsaved page in the same server process. For an existing workspace, set `GAZETTE_VSCODE_READ_ONLY_WORKSPACE` to its absolute path and `GAZETTE_VSCODE_NATIVE_PAGE` to one selected authored page; that mode changes only isolated editor buffers and checks that saved source bytes remain unchanged. The workspace must already use discovery without legacy source lists. These checks establish the local development host behavior; VSIX installation and other hosts need their own qualification.
