# Install Gazette from source

Build the `gazette` executable on Linux or macOS. The same binary can check configuration, render a publication and run the local editor or agent server.

For agent use, build the executable below, put its directory on your shell's `PATH`, then open your publication folder and [install the Gazette plugin](../integrations/agents/README.md). One plugin directory supplies the Claude Code and Codex skill and MCP configuration; Claude also receives explicit language-server discovery settings. The integration is under qualification and currently requires a source build.

## What a build needs

Start with a local Gazette source checkout and the usual C build tools. Install the toolchain in `rust-toolchain.toml`: Rust 1.94.0, including `rustfmt` and `clippy`. Rustup installs this version when you run Cargo from the checkout. If you manage Rust another way, install that version yourself.

::::steps

:::step "Build the executable"

From the Gazette repository root, run:

```sh
cargo build --locked --release --bin gazette
```

Cargo writes `target/release/gazette` unless your Cargo configuration chooses another target directory or target triple.

:::

:::step "Check that it runs"

Set these two variables so the publication tutorial can find your checkout and executable:

```sh
export GAZETTE_SOURCE="$(pwd -P)"
export GAZETTE="$GAZETTE_SOURCE/target/release/gazette"
"$GAZETTE" --help
```

You should see Gazette's command help. If you use a custom Cargo target directory, set `GAZETTE` to the executable path printed by Cargo with `--message-format=json`.

:::

:::step "Use the build"

To start a publication, follow [Your first publication](https://rawthoughts.in/gazette/first-publication/#start-the-publication). Its setup script needs Python 3. To embed Gazette in an application, see [Gazette architecture](architecture.md#share-behavior-across-interfaces).

:::

::::

For other commands, use `gazette --help` or `gazette <command> --help`.

## Contributor checks {#checks-before-a-change}

If you are changing Gazette, install Git with `git-http-backend`, OpenSSL, Python 3, curl and Node 22.18 or newer. Run these checks from the repository root, passing the tool paths to the test suite:

```sh
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo test --workspace --all-targets --locked --no-run
GAZETTE_TEST_NODE="$(node --print 'process.execPath')" \
GAZETTE_TEST_CURL="$(command -v curl)" \
GAZETTE_TEST_GIT="$(command -v git)" \
GAZETTE_TEST_OPENSSL="$(command -v openssl)" \
GAZETTE_TEST_PYTHON="$(command -v python3)" \
cargo test --workspace --locked --no-fail-fast -- --test-threads=4
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --locked --no-deps
python3 tools/generate_distribution_evidence.py --check
```

The last command validates the locked inputs and builds the dependency inventory in memory. Release and editor packaging generate notices beside their artifacts; see [distribution generation](#generate-distribution-notices) for local output and verification commands.

Math tests require the Node version above. Tests for generators, Graphviz, Chrome and the native addon run separately with the executable inputs described in their guides. The source build does not install those optional tools.

## Generate distribution notices

The offline [generator](../tools/generate_distribution_evidence.py) reads the [distribution configuration](../tools/distribution-evidence.toml), exact Cargo, npm, Python and Go locks, checksum-verified assets and bundled libgit2/SQLite licence records. It generates `NOTICE`, `THIRD_PARTY_NOTICES.md` and `SBOM.spdx.json` for the release or editor package. Vendor licences remain with their source.

Validate the inputs and generated model without writing files:

```sh
cargo fetch --locked
python3 tools/generate_distribution_evidence.py --check
python3 -m unittest discover -s tools/tests -p 'test_distribution_evidence.py'
```

To inspect or package the output, choose a temporary or release directory:

```sh
python3 tools/generate_distribution_evidence.py --output-root target/distribution
python3 tools/generate_distribution_evidence.py --check --output-root target/distribution
```

> [!IMPORTANT]
>
> **Generate from the release source.** Packaging generates fresh notices from the locked inputs; no copy is committed. `--check` without a destination validates inputs in memory; with a destination it also requires every generated file to match. A lock, asset or licence checksum mismatch still fails either check.

Local Cargo packages use their repository-relative manifest path, name and version for inventory identity. External packages retain their full Cargo source identifier, name and version, including Git revisions and distinct registries. Cargo's absolute local package IDs remain internal graph keys only. Moving the same checkout therefore preserves the inventory without merging packages from different homes or sources. [Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html) documents these fields. Exact upstream licence spellings remain in the notices and comments; only known legacy slash spellings are normalized in SPDX expressions.

The portable workflow generates into the runner's temporary directory and checks that output. The tagged-release job packages freshly generated source notices and inventory alongside the executable, then attaches a separate binary SBOM and checksums.

## Prepare a release binary

The tagged-release workflow builds with `cargo auditable` on Linux and macOS. It remaps source paths beneath the build user's home to `/build-home`, then the checkout to `gazette`, so the more specific checkout mapping takes precedence. It also strips debug information, including linker entries that source-path remapping does not cover. Before assembling the archive, it rejects a binary containing either original path. This check covers those two literal prefixes; it does not establish that every embedded string is suitable for publication.

A local source build above does not apply these release settings. Run the workflow and check its actual release artifact before distributing it.
