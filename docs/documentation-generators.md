# Run a documentation generator {#native-documentation-generator-qualification}

When a library changes, its API reference needs to change with it. Gazette can run the library's documentation tool and place the result beside its handwritten guides. TypeDoc reads TypeScript declarations; Rustdoc reads Rust code and comments. Gazette supplies their selected files, runs the configured command and assembles its output into the documentation site.

This guide is for the developer setting up that build. If you already have generated reference on disk, start with [Choose an input format](mintlify-formats.md) and assemble it directly. To add a handwritten guide first, follow [Add a manual to Field Notes](mintlify.md#configured-local-assembly).

## Follow the source into the reference

Suppose your library adds an optional argument to a function. You update its declaration and explain the new argument in its code comment. The generator reads those changes and produces the signature and explanation; you keep the longer example in the handwritten guide. Both need to describe the same version of the library, so select one prepared source tree for them, or compare their selected bytes before assembly.

The TypeDoc example below has three results you can inspect: the captured source files, the generator's `reference.json`, and a Mintlify directory containing that reference and `docs.json`. Gazette keeps the generator's files even if the later assembly fails. You can correct the assembly and reuse those files without running TypeDoc again.

| You have | Start here |
| --- | --- |
| API output already on disk | [Choose an input format](mintlify-formats.md). You can assemble these files without running the generator again. |
| A Rust library | [Prepare and run Rustdoc](#configure-a-rustdoc-job). |
| A TypeScript project | [Configure and run a TypeDoc job](#configure-a-typedoc-job). |
| TypeScript declarations that need to preserve qualifiers | [Use the Markdown formatter](#local-typedoc-markdown-formatter). |
| A changed generator or adapter | [Compare the generated reference with its source](#qualify-a-generator-integration). |

Install the generator and its dependencies before running the job.

> [!CAUTION]
>
> **Review the selected tools and build scripts.** They run with the process’s permissions. The staging directory limits where Gazette puts files, not what a generator can do.

## Configure a TypeDoc job

This example assembles one library's TypeDoc JSON. You need a built Gazette, an installed TypeDoc toolchain and a source directory containing `typedoc.json` plus its complete selected project files. A minimal options file is:

```json
{
  "entryPoints": ["src/model.ts", "src/api.ts"],
  "tsconfig": "tsconfig.json",
  "disableGit": true,
  "disableSources": true,
  "treatWarningsAsErrors": true
}
```

Use your library's actual entrypoints, TypeScript configuration and required local imports. Include every file the generator needs in the selected source. The paths in `typedoc.json` are relative to that captured input; they must not depend on files elsewhere in your checkout.

The supported TypeDoc profile uses TypeDoc 0.28.20 and TypeScript 5.9.3. Install the complete tool and its dependencies outside the source directory. When selecting another version, compare the generated declarations and build the resulting documentation before using it.

Save the following as `reference-workspace.toml`. The source names your library's files; the navigation entry places its generated reference under `library/reference`. The recipe gives Gazette the command to run. Replace the absolute paths and fill in the executable observations described just after the example:

```toml
[[principals]]
id = "owner:library"
kind = "person"

[[sites]]
id = "site:library"
publisher = "owner:library"
deployable = false

[[projects]]
id = "project:library"
lifecycle = "active"
visibility = "public"
availability = "available"

[[sources]]
id = "source:library"
location = { kind = "filesystem", root = "/absolute/library-input" }

[[brands]]
id = "brand:library"
name = "Library"

[[brands.publications]]
site = "site:library"

[brands.publications.documentation_repository]
name = "library-docs"
path = "/absolute/intended-docs-repository"

[brands.publications.documentation_repository.build]
target = { kind = "mintlify", theme = "mint", primary_color = "#315c4a" }

[[brands.publications.documentation_repository.build.modules]]
project = "project:library"
slug = "library"
title = "Library"

[[brands.publications.documentation_repository.build.modules.navigation]]
kind = "reference"
slug = "reference"
title = "API reference"

[brands.publications.documentation_repository.build.modules.navigation.input]
kind = "generated"
source = "source:library"

[brands.publications.documentation_repository.build.modules.navigation.input.recipe]
program = "/absolute/node"
executable = { bytes = 123456, sha256 = "<node-sha256>" }
arguments = [
  { kind = "literal", value = "/absolute/tool/node_modules/typedoc/bin/typedoc" },
  { kind = "literal", value = "--options" },
  { kind = "input", path = "typedoc.json" },
  { kind = "literal", value = "--json" },
  { kind = "output" },
]
environment = {}
version_arguments = ["/absolute/tool/node_modules/typedoc/bin/typedoc", "--version"]
expected_version_sha256 = "<complete-version-stdout-sha256>"
output = { kind = "file", path = "reference.json" }
format = "typedoc-json"
```

`input` selects `typedoc.json` in the captured source, and `output` names a new file in the job's output directory. Gazette passes the literal arguments directly, without a shell or PATH lookup. This example captures the files currently on disk. To require a particular commit, choose a [repository source](workspace-sources.md#acquire-pinned-documentation-for-a-site); a local capture alone does not establish a Git revision.

### Record the tool you intend to run

The recipe records Node's byte count and SHA-256, plus the SHA-256 of TypeDoc's version output. Before filling those fields, select the exact Node executable, TypeDoc script and installed dependency tree. Run the script's `--version` with the recipe's explicit environment and retain all its stdout bytes, including newlines. Hash those bytes for `expected_version_sha256`.

These observations let Gazette stop if the executable or version output changes between setup and generation. They do not approve the installed packages: review those separately, including plugins and build scripts. [Recipe fields](#configured-execution-and-retained-evidence) explains machine-bound paths, environments and validation commands when your integration needs them.

### Run and interpret the result

Create separate existing private parents for generator jobs and target stages, outside every source directory selected for this documentation build. Gazette refuses overlapping sources and output locations, including an ignored `target/` directory inside a selected repository. Then invoke the selected Gazette:

```sh
gazette assemble-docs --local --site site:library --allow-execute \
  --generator-parent /absolute/private/generator-jobs \
  --output-parent /absolute/private/documentation-stages \
  --generator-timeout-ms 30000 --max-generator-jobs 1 \
  < reference-workspace.toml > documentation-result.json
```

A successful report has `status: "documentation-assembled-local"`:

- `jobs[0]` identifies the retained generator input/output stages, version observation, generation invocation, complete tree and selected artifact.
- `directory` is the new Mintlify target stage. For this example it contains `docs.json` and the original TypeDoc JSON at `artifacts/library/reference.json`.
- `repository.path` is the configured intended destination. Assembly does not create or replace that repository.

Open the target with your local documentation provider and inspect symbols, examples and links. For a real library, compare the complete public declarations, not just a few rendered signatures. [Target formats](mintlify-formats.md) describe what the adapter accepts; [Mintlify integration](mintlify.md) explains assembling manual sections alongside this reference.

### Reuse a generator result {#reuse-retained-output}

To assemble the same result again without running TypeDoc, bind a source to the original job's retained output root and replace the navigation input with:

```toml
input = { kind = "artifact", source = "source:retained-reference", selection = { kind = "file", path = "reference.json" }, format = "typedoc-json" }
```

Retain and check the original artifact/tree identity when selecting that root. Use `selection = { kind = "directory", path = "reference" }` for a directory format; omitting `path` explicitly selects the entire capture root. A file or directory declaration must match the retained entry. Neither target assembly nor repeated adaptation runs the generator again.

## Configure a Rustdoc job

Use the [Rustdoc provider](../tools/documentation-reference/rustdoc.py) through the same configured generator runner. It builds the selected library with Cargo, uses Cargo’s resolved package identities and actual compiler artifacts to locate each dependency version, and gives the resulting HTML to the existing Rustdoc converter. Renamed dependencies and multiple versions of one crate keep their own links.

> [!IMPORTANT]
>
> **Select a complete installed nightly toolchain.** This provider uses Rustdoc’s unstable external-root options. Configure absolute paths to Cargo, rustc and rustdoc from that toolchain, and to Python 3.12 or newer. Provision Cargo’s locked dependencies beforehand; generation runs with `--offline --locked` and fails if a dependency is unavailable.

Put `rustdoc.json` in the selected source tree. For a library whose package manifest is at the root:

```json
{
  "manifest": "Cargo.toml"
}
```

`manifest` must select one package with a library target. Include its workspace manifests, lockfile, local dependencies and source files in the capture. A workspace-only manifest is ambiguous and is refused. For Gazette’s Rust SDK, use `"manifest": "sdk/rust/Cargo.toml"` and `"features": ["native-local"]` over the complete selected repository. `no_default_features` defaults to `false`; set it explicitly when that is the intended API.

Crates from crates.io receive version-specific `https://docs.rs/<package>/<version>/` roots. For another registry, a Git dependency or a local package with published reference, supply `dependency_roots` in the same JSON object:

```json
{
  "dependency_roots": {
    "example-dependency@1.2.3": "https://docs.example.org/example-dependency/1.2.3/"
  }
}
```

A `name@version` key must identify exactly one resolved package. When two sources use that same name and version, use Cargo’s complete package ID. Roots must be absolute HTTPS directory URLs. Gazette does not infer a documentation address for unpublished local or Git dependencies. Unresolved local links fail the subsequent target assembly; a URL declaration does not prove that an external host serves it.

In the workspace example above, keep the source, module and reference selection and replace the recipe with:

```toml
program = "/absolute/python3"
executable = { bytes = 123456, sha256 = "<python-executable-sha256>" }
arguments = [
  { kind = "literal", value = "-B" },
  { kind = "path", path = "machine:reference-tools/rustdoc.py" },
  { kind = "literal", value = "--cargo" },
  { kind = "path", path = "machine:nightly/bin/cargo" },
  { kind = "literal", value = "--rustc" },
  { kind = "path", path = "machine:nightly/bin/rustc" },
  { kind = "literal", value = "--rustdoc" },
  { kind = "path", path = "machine:nightly/bin/rustdoc" },
  { kind = "literal", value = "--input" },
  { kind = "input-root" },
  { kind = "literal", value = "--output" },
  { kind = "output-root" },
  { kind = "literal", value = "--config" },
  { kind = "input", path = "rustdoc.json" },
]
environment = { PATH = "/absolute/nightly/bin:/usr/bin:/bin", CARGO_HOME = "/absolute/provisioned-cargo-home" }
version_arguments = ["-B", { path = "machine:reference-tools/rustdoc.py" }, "--version"]
expected_version_sha256 = "<complete-provider-version-stdout-sha256>"
output = { kind = "directory", path = "reference/example_library" }
format = "rustdoc-html"
```

Bind `reference-tools` to the installed provider directory and `nightly` to the selected toolchain through the workspace’s machine file. The explicit `--cargo`, `--rustc` and `--rustdoc` arguments keep those machine locations out of captured source options. Existing JSON options may still name these tools; if a tool is selected in both places, every declaration must name the same path. Conflicting JSON or repeated command-line selections fail before preparation. Missing selections fail rather than consulting PATH.

Replace `example_library` with the library’s Rust crate name, such as `gazette_sdk`. Resolve the Python executable to its actual file before recording its byte count and SHA-256; the runner refuses an executable symlink. Observe `/absolute/python3 -B /absolute/tools/rustdoc.py --version` for the version digest. Select any linker and native build prerequisites through the recipe’s explicit environment as well. The provider records the selected Cargo, rustc and rustdoc versions and executable digests, and refuses replacement during the job.

A recipe with machine coordinates must load its owning workspace and machine binding. Put `machine_binding = "machine.local.toml"` at the root of `reference-workspace.toml`, with the tool roots in that machine file. Supply the local workspace selector through `--workspace-json`; a detached TOML buffer has no machine roots:

```sh
printf '%s\n' '{"kind":"local","root":"/absolute/workspace","workspace":"reference-workspace.toml"}' |
  gazette assemble-docs --workspace-json --local --site site:library --allow-execute \
    --generator-parent /absolute/private/generator-jobs \
    --output-parent /absolute/private/documentation-stages \
    --generator-timeout-ms 300000 --max-entries 40000 > documentation-result.json
```

This example allows a five-minute invocation (the runner's maximum) and up to 40,000 entries in the generated output set and assembled tree. Generator inputs have independent CLI limits:

| Flag | Default | Counts |
| --- | --- | --- |
| `--max-generator-input-entries` | 10,000 | Entries across all generator input copies, including directories and hidden files. |
| `--max-generator-input-total-bytes` | 536,870,912 (512 MiB) | Combined file bytes across all input copies. |
| `--max-generator-input-file-bytes` | 67,108,864 (64 MiB) | Bytes in one input file. |
| `--max-generator-input-depth` | 64 | Descendant levels within each input tree; immediate children have depth one. |

A source used by two jobs counts twice against the entry and total-byte allowances. For example, add `--max-generator-input-entries 20000` when repeated copies need that allowance. Zero does not restore the default.

Increasing `--max-entries` changes output limits only; `--max-source-bytes` bounds the workspace input on stdin. Source acquisition keeps its separate native defaults, including 10,000 entries; larger acquisition limits remain available through MCP `assemble_docs` and the SDK. These bounds limit a run; they cannot guarantee that it completes.

### Inspect the original, prepared and generated files

The job’s input stage keeps the original captured source. Its output stage contains:

- `prepared/`: the same selected files, with private, hidden `cfg(doc)` dependency imports appended only to the chosen library root. Original declaration and example bytes remain intact. Source-derived aliases avoid existing names; a detected collision fails preparation.
- `reference/`: native Rustdoc HTML, runtime companions and source views. The configured crate directory is the reference selected for assembly.
- `preparation.json`: original and prepared source identities, exact dependency package/version/artifact identities and link roots, provider identity, tool observations and each native invocation’s result.
- Cargo and tool output files for diagnosing a failure. Cargo’s disposable compilation products are removed after successful reference capture; failures keep their residue in the same job output.

The existing native runner checks the original input before and after the invocation. The provider also checks that preparation changed only the library root and that neither source copy changed during generation. Warnings, including broken comment links, fail Rustdoc. The shared converter then checks the complete selected reference’s local pages and fragments. Keep these checks distinct from comparing the public API and reading the assembled reference.

Reuse a completed `reference/` tree through [the artifact-input path](#reuse-retained-output); preparation does not run again during that assembly. Keep `prepared/`, logs and `preparation.json` with the private job result. They are not manual pages or publication inputs.

## What the runner reads and keeps {#configured-execution-and-retained-evidence}

| Field | What it tells the runner |
| --- | --- |
| `program`, `executable`, `version_arguments`, `expected_version_sha256` | Which executable bytes and version output to require. A mismatch stops before generation. Pin imported packages and plugins separately. |
| `arguments` | `literal` passes a value; `path` resolves an explicit tool path through the workspace and machine binding; `input` names a file; `input-root` names the full working directory; `output` names the result; `output-root` names the output stage. No shell expands these values, and the runner does not guess from the language. |
| `environment` | The child's complete environment. Strings stay literal; an explicit `{ path = "machine:tools/packages" }` value resolves through the captured machine binding. The runner inherits no environment and does not search PATH. |
| `output`, `format` | Which file or directory to use and which adapter format should consume it. |
| `validation_arguments` | Optionally run the same executable over the output with `output` or `output-root`. A nonzero result, changed files or extra files fail the job. The validator cannot replace generated bytes. |

The recipe's `program` accepts an absolute executable path, a workspace-relative coordinate or `machine:name/path`, using the same [machine binding](workspace-sources.md#bind-named-roots-on-each-machine) as source roots. For example, select `program = "machine:python/bin/python3"` and `environment = { PYTHONPATH = { path = "machine:packages" } }` when an interpreter and its package directory live outside the captured project. Each typed environment value names one path; strings such as `"machine:packages"` stay literal, with no interpolation or path-list expansion. The selected executable must still pass the native file and digest checks; a symlink is not a replacement for the real executable selection.

All selected generator coordinates bind before the first job stage is created. The binding comes from the loaded workspace's captured machine file, never from the process directory or a later file read. Supplied TOML buffers have no native workspace or machine roots, so they require absolute executable coordinates and absolute typed environment paths. Generator `input` and `input-root` arguments still name the retained source stage; they are not machine paths. Version arguments accept literal strings and explicit `{ path = "machine:tools/cli.js" }` values. Generation and validation arguments use `{ kind = "path", path = "machine:tools/plugin.mjs" }` for the same resolver. Unknown roots, missing workspace context, malformed coordinates and expanded commands above the native limits fail before any job stage is created. Strings that merely contain `machine:` remain literal; no field interpolates paths. The exact executable and version-output checks remain required.

Gazette checks job count, permissions and the target's names, presentation and formats before staging. Every job starts with the captured inputs and an empty output directory. Gazette checks the inputs and version, runs the generator once, then captures its complete result. Repeated input stages and all generated outputs count against the shared limits.

### Diagnose a failed job

| Failure | What to do |
| --- | --- |
| Executable or version mismatch | Check the selected tool and all its installed dependencies before recording a replacement in the recipe. |
| Generator exit or timeout | Inspect its exact invocation, retained stages and native cause. A successful prefix remains evidence. |
| Missing selected result or file/directory mismatch | Compare the recipe with the actual captured tree; preserve the residue. |
| Validator failure or changed output | Keep generation and validation results separately. Correct the source/tool selection for a new deliberate job. |
| Target adaptation or staging failure | Inspect the retained artifact first. A completed generator need not run again merely because the later target failed. |
| Staging refusal before any job starts | Check that both parent directories exist outside all selected source roots. An ignored build directory inside a source repository still overlaps it. |
| Result delivery lost or too large | Keep the original request and stage paths. Inspect those directories before trying again: the generator may have run even though you received no result. |

When the full report will not fit, Gazette returns a smaller failure report with the original stage paths, completed or active commands and their identities. It may omit version text while keeping its digest and byte count. If the target returns `Invalid`, read `failure.target_diagnostic`: `context` locates the problem in the build, `reason` describes it, and omission flags say whether details were left out to fit the response. This diagnostic contains no artifact body and opens no additional file. If the CLI cannot write stdout, it reports failure and attempts to preserve the smaller report on stderr.

Before distributing generated files, inspect them for private source paths and other information you did not intend to publish. Time, byte and entry limits help control a run's size; tools and plugins still need the review described at the start of this guide.

## Local TypeDoc Markdown formatter

Use this formatter when direct TypeDoc JSON conversion loses detail your API needs, such as `readonly` in an anonymous declaration or index signature. It uses TypeDoc's [local-plugin and theme mechanism](https://typedoc-plugin-markdown.org/docs/customization) to produce Markdown. Page titles remain escaped plain text with generic constraints and defaults intact; TypeDoc supplies the model, routes, comments and body links. The two adapted type-formatting partials keep their upstream MIT notice, and the package lock records the installed tool dependencies.

Copy the complete [`tools/documentation-reference`](../tools/documentation-reference/package.json) source directory to a private tool directory outside the manual's captured source. Retain `package.json`, `package-lock.json`, `plugin.mjs`, `provider.mjs`, both `partials/*.mjs`, `upstream.json`, `UPSTREAM-LICENSE` and `test/`. The plugin imports the provider helper and derived partials; copying just the manifests is incomplete. Exclude any existing `node_modules`, then provision the copy explicitly:

```sh
cd /absolute/private/reference-formatter
/absolute/node /absolute/npm-cli.js ci --ignore-scripts --no-audit --no-fund
/absolute/node --test test/*.test.mjs
```

Use a Node version within the tool's supported range, then record the exact executable and version output in the recipe as above. Install dependencies before generation; Gazette does not run npm for you. Run the formatter's own tests after changing it.

For retained TypeDoc JSON, select the exact original model and any source-file companions it names. The merge loader may require those captured declarations even when source links are disabled. For Mintlify, hide the producer's repeated banner and breadcrumbs while retaining its H1 for the target's title and original anchor. An options file can select:

```json
{
  "entryPointStrategy": "merge",
  "entryPoints": ["reference.json"],
  "theme": "contract-markdown",
  "hidePageHeader": true,
  "hideBreadcrumbs": true,
  "hidePageTitle": false,
  "readme": "none",
  "disableSources": true,
  "disableGit": true,
  "treatWarningsAsErrors": true,
  "fileExtension": ".md",
  "entryFileName": "index",
  "router": "member",
  "expandObjects": true,
  "expandParameters": true,
  "useCodeBlocks": true
}
```

`useCodeBlocks: true` puts declaration signatures in fenced code blocks. Setting it to `false` uses inline code and blockquotes; the local plugin preserves qualifiers in either form but does not override that presentation choice.

Pass that options file as an `Input` argument to the pinned TypeDoc script, and its output directory as `Output`, through the ordinary configured native runner. Bind `formatter` to the complete installed tool directory and pass both plugins through TypeDoc's repeated `--plugin` option:

```toml
arguments = [
  { kind = "path", path = "machine:formatter/node_modules/typedoc/bin/typedoc" },
  { kind = "literal", value = "--options" },
  { kind = "input", path = "typedoc.json" },
  { kind = "literal", value = "--plugin" },
  { kind = "path", path = "machine:formatter/node_modules/typedoc-plugin-markdown/dist/index.js" },
  { kind = "literal", value = "--plugin" },
  { kind = "path", path = "machine:formatter/plugin.mjs" },
  { kind = "literal", value = "--out" },
  { kind = "output" },
]
version_arguments = [{ path = "machine:formatter/node_modules/typedoc/bin/typedoc" }, "--version"]
```

Keep plugin locations out of the captured options. TypeDoc includes its TypeScript installation path in version output, so relocating the tool requires a new version observation and digest even when its files are identical. Import the resulting directory as `format = "markdown"`. Keep the original JSON and each formatter run's report and output. To change presentation, change the formatter and run a new job; leave the earlier output available for comparison. Reassembling an unchanged Markdown directory needs no generator run.

Compare the complete public API with its original declarations, including readonly and optional modifiers, generic constraints and defaults, overloads, and nested mapped and conditional types. Build the documentation, follow its links and navigation, and read the signatures as they appear on the page. A successful build can still show an inaccurate signature. Directory Markdown navigation defaults to lexical order. Use the format's [pages and directory selections](mintlify-formats.md#generated-markdown) to name chapters and derive their symbol groups from generated directories. Gazette checks that the expanded navigation includes every page exactly once.

## Check the generated reference {#qualify-a-generator-integration}

<a id="required-language-coverage"></a> <a id="five-generator-qualification"></a> <a id="reproducible-ubuntu-gate"></a> <a id="observed-routes"></a> <a id="native-typescript-qualification-fixture"></a> <a id="native-rust-proof"></a> <a id="static-rustdoc-target-proof"></a> <a id="integration-obligations"></a> <a id="native-swagger-generation-and-validation"></a>

Before adopting a generator or changing its version:

1. Compare every public symbol with the source declarations. Check signatures, readonly and optional modifiers, overloads and generic constraints.
2. Follow links between generated pages and from the manual into reference. Check the original fragment targets too.
3. Build the complete documentation target with its intended provider. Open representative long signatures, code samples and nested navigation in the browser.
4. Keep the generator request and original output so a later formatting change can be checked without guessing what the tool produced.

Use [the format reference](mintlify-formats.md) for each supported input profile and its limits. Compare the result with the API it describes before giving it to readers.
