# Working on Gazette

Gazette publishes a body of thought from the whole to its parts, and makes that structure what readers, search, feeds, JSON-LD and `llms.txt` index. It is a publication system built in Rust that combines authored content, publication configuration and Themes into websites and documentation, with navigation, search, feeds and structured data drawn from the same model. Its CLI, MCP server, language server and SDKs share compiler and application behavior.

## Start with the part being changed

Read the [README](README.md), [developer guide](docs/README.md) and [architecture](docs/architecture.md). `crates/` holds the content model, compiler, application and ports; `adapters/` implements I/O and rendering; `sdk/` and `interfaces/` expose that behavior. Keep publication meaning in the compiler and application, and presentation in Themes. Workspaces supply content selections and delivery settings.

Use the [MCP](docs/mcp.md) and [language-server](docs/lsp.md) contracts when changing agent or editor behavior. The built executable provides `gazette mcp` and `gazette lsp`; the [VS Code guide](docs/vscode.md) covers the editor integration. For code navigation, use this repository's CodeGraph index when available, confirm the returned paths, and check the current source before editing.

Use [Clear writing](https://github.com/getHarshOnline/pale/blob/main/packages/publication/clear-writing.md) in Pale's Publication guidance for comments, help, errors, commits and release notes.

## Build and check the affected component

Follow [build and test setup](docs/install.md) for the pinned Rust toolchain and test prerequisites. From the repository root:

```sh
cargo build --locked --release --bin gazette
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Run the workspace test command in [build and test setup](docs/install.md), including its `GAZETTE_TEST_*` executable bindings. Generator, browser and native-addon checks run separately as described in [dependency documentation](docs/maintain/dependencies.md). Choose checks for the changed behavior and its consumers; a source build alone does not exercise those integrations.

Keep tests with their owning crates and preserve the dependency checks. For new integration tests, use one binary per crate, with `tests/main.rs` declaring case modules; exercise wiring through the CLI. Consolidating existing suites is separate product work.

When dependencies or distribution inputs change, run `python3 tools/generate_distribution_evidence.py --check` to validate the locked inputs without writing files. Generate notices and inventories with `--output-root` into a release or temporary directory; they are built by the release job and never committed. Keep vendor code and its required notices in their existing form.

Remove code only when reachability, supported contracts and ownership establish that it is dead. A whole-goal size target or lack of current use does not justify removing a supported feature. Textual repetition alone does not establish a duplication defect.

<!-- BEGIN shared repository rules -->

- Review a source file above 1,000 lines and split it by concern before 1,500.
- Keep Rust tests under `tests/`; keep Go's `_test.go` files beside their package.
- Finish a goal with the codebase the same size or smaller unless a requested feature requires growth. Preserve the behaviour, tests, cases and examples.
- Every engine must build from a fresh clone on Linux and macOS.
- Keep generated API reference and build output out of Git; change their source or generator.
- Write comments and API documentation for the code's reader. A crate or module comment first says what it is for in the system, what it builds on and what builds on it, then its purpose, inputs, results and useful edge cases, with the purpose written problem-first in the present tense: the problem it solves and what solves it today. One plain line after that opening says what is not built yet, as the problem now in front of it, and a negative appears only where a reader would expect the opposite: a limit stated as a property of what exists stays ("it runs on loopback with no caller authentication"), while a negative measured against the finished design is a hedge. Keep them self-contained, with links inside the repository.
- Push each repository as one commit whose whole message is `init`, with Harsh as author and committer under the account the folder uses and no `Co-Authored-By` or `Claude-Session` trailers; later commits stay short and plain.
- Describe the present design and research. The repository neither links nor ships lineage: earlier papers, predecessor versions, history tags and the commits behind them.
- Public repository files, including generated text, link within the repository or to public addresses their readers can open.
- A README is the repository's front door: it carries the product account for the technical reader who arrives there, opening with the product's plain name and the problem it solves, in the words its published pages use.
- Name every operation, command, tool, type, field and variable so a reader without its author's context can tell what it does, what it enables and when to use it; docstrings say the same. Keep full-field record schemas for the engine's consistency, named in plain words like everything else, and author each surface's schemas separately, one per intent, in its caller's terms: SDKs and APIs, the command line, the language server and the agent protocols (MCP, A2A, ACP) are different surfaces with different callers, and the agent protocols compose intents over the operations the others expose.
- Write each Markdown paragraph on one source line. Run the repository's formatter with `proseWrap: never` on changed Markdown.

<!-- END shared repository rules -->

## Before handing back

- Does the change preserve the publication model and the affected outputs?
- Did the relevant checks run, with failures or untested integrations explained?
- Do the changed documentation and links describe what the current code does?
