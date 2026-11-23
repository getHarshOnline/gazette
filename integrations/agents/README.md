# Gazette for agents

Gazette meets each actor where it works, with the same publication model behind every mode: a person writing in an editor arrives through the language server, an agent helping in Claude Code or Codex through the MCP server and its skill, and a build or integration through the command line and SDKs.

Open a publication folder containing `gazette.toml`, then connect Gazette to Claude Code or Codex. This plugin supplies one Gazette skill and one local MCP server to both hosts. Claude Code also receives language-server settings for configured Markdown, TOML and notebook files. Gazette reads the sources declared by the publication; no generated file list is needed.

This is a source-checkout integration under qualification. The package does not install the Gazette executable. Installation and removal have been checked in isolated Claude Code and Codex homes. Claude connected to the installed MCP server; Codex discovered the installed skill and invoked a Gazette tool. Real-model use, Claude language-server feedback and matching page diagnostics across the interfaces still need qualification.

## Prepare the executable

[Build Gazette](../../docs/install.md#what-a-build-needs), then make that build available as `gazette` on the `PATH` of the shell that starts your agent. From the Gazette checkout, a source-build example is:

```sh
cargo build --locked --release --bin gazette
export PATH="$(pwd -P)/target/release:$PATH"
gazette --help
```

Keep this shell open and change into the publication folder before starting the agent. If Cargo uses a different target directory, use that build's directory instead. No release installer or public marketplace listing is implied by these instructions.

## Claude Code

For a session-only trial, start Claude from the publication folder with the plugin directory in your Gazette checkout:

```sh
claude --plugin-dir /path/to/gazette/integrations/agents
```

For an installed project plugin, add the checkout's local marketplace and select the plugin:

```sh
claude plugin marketplace add /path/to/gazette --scope project
claude plugin install gazette@gazette-local --scope project
```

Start a new Claude session in the publication folder. Check `/mcp` for the Gazette connection and `/plugin` for loading errors. The plugin passes explicit discovery and read settings to `gazette lsp`; an empty initialization request remains unsupported. The directory must contain `gazette.toml`. Use a separate project scope for unrelated repositories, and resolve any other plugin claiming the same file extensions before relying on feedback.

Ask Claude to check the publication and explain a diagnostic. To exercise language-server feedback, edit a disposable page to reference a missing identity, confirm the reported location and repair it. MCP and LSP currently use separate sessions, so a saved-file check cannot establish what is in an unsaved editor buffer.

Remove an installed project plugin from that publication folder with:

```sh
claude plugin uninstall gazette@gazette-local --scope project
```

## Codex

Codex supports the same skill and MCP configuration through its manifest. The checkout's `.claude-plugin/marketplace.json` uses Codex's supported compatibility layout. With a Codex CLI that provides `plugin marketplace` and `plugin add`:

```sh
codex plugin marketplace add /path/to/gazette
codex plugin add gazette@gazette-local
```

Start a new session in the publication folder and confirm that Gazette's skill and MCP tools are available. Ask it to check that publication, then inspect the workspace and diagnostics in the actual result. Codex's manifest does not register Claude's language-server entry; use the MCP tools for Gazette feedback in Codex.

Remove the installed plugin with:

```sh
codex plugin remove gazette@gazette-local
```

These installation and removal commands were checked with Codex CLI 0.169.0-alpha.11.1 and Claude Code 2.1.325. The Codex host check invoked `check_workspace` on supplied configuration without a model turn; it does not establish automatic skill selection or a complete publication workflow. Installing alongside a manually registered Gazette server can expose two separate sessions; resolve that duplication deliberately when migrating an existing setup.

## Integration details

The [MCP reference](../../docs/mcp.md) describes tool discovery and operation inputs. [Editor discovery](../../docs/editor-discovery.md) covers the language server's source selection, read permission and behavior during configuration repair. [The LSP contract](../../docs/lsp.md) describes synchronization and query results. No plugin setting grants build, network or publication permission to those operations.

The host formats are described in [Claude's plugin reference](https://code.claude.com/docs/en/plugins-reference) and [OpenAI's plugin packaging guide](https://developers.openai.com/plugins/build/plugins). The skill and server settings stay in this directory; the repository-root marketplace only makes that package discoverable.
