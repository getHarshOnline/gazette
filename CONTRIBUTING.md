# Contributing to Gazette

Gazette's compiler and application define publication behavior once for the CLI, MCP server, language server and SDKs. Start with the [architecture](docs/architecture.md) to find the owner of a change, then follow [build and test setup](docs/install.md) for the pinned toolchain and executable bindings.

Outside contributions are not yet being accepted. Opening them depends on review of the contributor agreement and an acceptance mechanism. This does not change the rights granted by [Gazette's licence](LICENSE).

## Change the owner and its callers

Keep content meaning, routes and publication relationships in the compiler and application. Adapters implement external effects; interfaces expose the shared behavior. Themes own presentation and workspaces supply selections. Update the affected callers, examples and tests together when a contract changes.

Before the first release, replace obsolete names and update their callers in the same change; do not add compatibility aliases merely to preserve an unreleased spelling. Deliberate migration aliases have a separate purpose: keep their documented reasons and the behavior they support. Publication routes that have actually been released also retain their required aliases; the pre-release rule does not remove that supported capability.

Keep API documentation with the declaration that owns it. Explain inputs, results, effects, limits and useful failure cases for the caller. Generated reference comes from those declarations; change its source or generator rather than generated output. Write one Markdown paragraph per source line and format changed Markdown with `proseWrap: never`. Repository documentation links within this checkout or to public addresses its readers can open.

## Check the affected behavior

Run the [contributor checks](docs/install.md#checks-before-a-change) for the affected packages and their consumers. Include a regression that exercises a repaired failure where needed. A successful build does not establish that a generator, editor host or browser integration works; use the corresponding maintenance guide for those checks and report their actual scope.

Keep Rust tests under their owning package’s `tests/` directory. The [architecture tests](sdk/rust/tests/architecture.rs) check dependency direction, test placement, the editor’s selected adapters and the public SDK boundary. Exercise operations through their public API; interface regressions should cross the real executable or protocol boundary. For a new integration suite, follow the single-binary layout in [the repository instructions](AGENTS.md).

For a native operation change, follow the [CLI operation registry](interfaces/cli/src/command/metadata.rs) through MCP discovery and dispatch to the SDK. The [MCP maintenance guide](docs/maintain/mcp.md) identifies the executable coverage and behavioral tests. Keep checks using supplied configuration distinct from checks that acquire native sources and compose them.

| You are changing | Maintenance guide |
| --- | --- |
| MCP discovery, dispatch or authoring | [MCP](docs/maintain/mcp.md) |
| The VS Code extension | [VS Code](docs/maintain/vscode.md) |
| A documentation generator or target adapter | [Documentation generators](docs/maintain/documentation-generators.md) |
| Dependencies, provider selection or release evidence | [Dependency choices and qualification](docs/maintain/dependencies.md) |

For current advisory-check instructions, run `python3 tools/check_dependencies.py --help`. The [distribution guide](docs/install.md#generate-distribution-notices) explains how to validate locked inputs and generate notices in a release or temporary directory. Keep required vendor notices intact; a passing check supplies neither legal review nor release approval.

These maintenance chapters belong to the source checkout and are not selected as public developer-manual chapters. Keep source navigation here when adding one. Application integrations start in the [developer guide](docs/README.md).

## Build and release checks

Use the [local build and test setup](docs/install.md) on Linux and macOS. Run the separate [provider fixtures](docs/maintain/documentation-generators.md) for changes to documentation generators. Read the advisory reports for the exact dependency locks and use the [binary preparation guide](docs/install.md#prepare-a-release-binary) when distributing a build.
