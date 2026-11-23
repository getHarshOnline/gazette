# First publication example

This source-owned starter creates a persistent publication with an introductory collection, two notes, their explicit reading relationships and a small HTML theme. It leaves the workspace and authored pages available for editing. It is separate from the disposable compiler check in `examples/quickstart`.

The [product tutorial](https://rawthoughts.in/gazette/first-publication/) owns the complete create, edit and preview instructions. Developers need Python 3 to exercise this starter and the [source-built Gazette executable](../../docs/install.md#what-a-build-needs) to compile the result. The script is an example helper, not a Gazette CLI command.

Run `python3 examples/first-publication/create.py --help` from the repository root to inspect its input. It writes only a new explicitly named directory and refuses any existing destination. All source paths in the resulting workspace refer to that directory. It starts no listener, invokes no build and creates no Git, store or release state.

The publication declares `content:field-notes` as its home and sets its route roots to `/`. Each note keeps a stable `id`, a readable `slug` and its collection membership. Gazette derives the note's address beneath that collection and supplies its site from the configured content source. Prose links use complete identities such as `[[id:content:review-a-note|review the note]]`; changing a title leaves those identities and addresses unchanged. See [derived routes](../../docs/routing.md) for deliberate address changes and explicit exceptions.

Its editor settings enable [source discovery](../../docs/editor-discovery.md). The workspace selects its content and Theme manifest, so adding a page under `content/` requires no separate editor file list.

The theme is self-contained. It renders the native page body, supplied author names, citation text and downloads, article-owned Notes, collection children, ordered journey steps and member navigation. Citation and Notes use native disclosures without JavaScript. Selected JSON-LD is embedded from the page model. Its alias entry preserves earlier addresses when the reading structure changes. It requires no network service or existing site workspace.

After building the debug executable, run this from the repository root to create and render a disposable instance of the persistent starter:

```sh
python3 examples/first-publication/check.py --gazette "$PWD/target/debug/gazette"
```

The portable workflow is configured to run the same command on macOS and Linux; its first Linux run is still to do. You can supply another built executable with `--gazette /absolute/path/to/gazette`. The check exercises the real template and retained download, preserves one Notes body and confirms that an earlier rendered result does not change after a source edit. It also checks the starter and product-help addresses, a title edit that preserves the note’s identity and URL, a deliberate slug change whose incoming identity link resolves to the new address, and the two assembled manual pages with their checked relative heading link. It does not perform a native move or editor rename operation.

`configure_math.py` prepares create-only local typesetter settings and a successor request for this publication, preserving the current request's other selections. It reads the explicitly selected Gazette checkout, request and Node executable; it starts no process beyond querying that interpreter's version. The [math task](https://rawthoughts.in/gazette/typeset-mathematics/) owns profile selection, article changes and preview instructions.

`documentation/` supplies two native manual pages and a separate product-help collection for the [developer-documentation task](https://rawthoughts.in/gazette/publish-developer-documentation/). They explain the same title change for its author and for someone writing a note-processing tool. Their public source links form a two-way handoff; private preview mappings connect the actual Gazette and Mintlify listeners.
