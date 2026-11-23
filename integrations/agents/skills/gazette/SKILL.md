---
name: gazette
description: Check, edit and preview publications configured with Gazette. Use for Gazette workspaces, authored pages, publication links, Themes and compiler feedback; an unrelated Markdown file or website does not imply a Gazette workspace.
---

# Work on a Gazette publication

Gazette combines authored content, publication configuration and Themes. Start with the publication folder's `gazette.toml` and its declared sources; the plugin's installation directory is not the publication. Keep article identity, authorship and reading relationships in their owning declarations, and presentation in the selected Theme.

Discover the connected Gazette tools and use their current input schemas. For a publication check, use `check_workspace` with the selected workspace file or folder when its schema supports that input. A supplied `source` checks detached configuration; that result does not establish that the publication's pages are valid. Report the workspace actually checked and the file, line and missing identity from each diagnostic. Fix the owning declaration or reference, then rerun the affected check.

For an edit, inspect the current source and compiler feedback before changing it. Language-server diagnostics include unsaved text; a separate filesystem check sees saved files. Keep that distinction when explaining a result. Do not silently replace an unsaved editor context with a saved-file check. An exported context can be imported only through the server's supported format and separately permitted native reads.

For preview, inspect the available build and preview operations, follow their declared inputs, and wait for the returned preview handle to report readiness before opening its address. A local preview is not a published site. Use the user's existing authorization for the requested work; publishing, pushing and account changes require their own authorization. After uncertain delivery of an operation, inspect the returned handle or state before retrying it.

If Gazette's tools are unavailable, check whether `gazette --help` runs in the host's environment. If the executable is missing, explain that the plugin needs a local Gazette build and point to the [plugin setup](../../README.md); do not invent an installer or silently substitute another publication tool. A tool connection, a passing configuration check and a working rendered preview establish different results.
