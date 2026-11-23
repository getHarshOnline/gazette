# Generate API reference beside the developer guides

These tools generate reference from selected Rust or TypeScript code. Gazette’s configured generator runner retains the input and native output, then its documentation converter assembles the reference beside authored Markdown guides. The workspace chooses the modules, chapter order and navigation. Generated reference remains build output; it does not replace the guides or become checked-in source.

| Input | Provider | Guide |
| --- | --- | --- |
| Rust library | [`rustdoc.py`](rustdoc.py), using an explicitly selected nightly Cargo, rustc and Rustdoc | [Configure a Rustdoc job](../../docs/documentation-generators.md#configure-a-rustdoc-job) |
| TypeScript declarations | TypeDoc JSON, or the pinned TypeDoc Markdown plugin with this directory’s formatter | [Configure a TypeDoc job](../../docs/documentation-generators.md#configure-a-typedoc-job) and [preserve declaration qualifiers](../../docs/documentation-generators.md#local-typedoc-markdown-formatter) |

## Generate Rust reference

Use `rustdoc.py` through the configured runner with a complete captured source tree, its lockfile and a `rustdoc.json` selecting one library package. Pass installed tools through explicit `--cargo`, `--rustc` and `--rustdoc` arguments when their paths come from machine bindings. Existing tool selections in JSON remain supported; conflicting declarations fail before preparation. Python 3.12 or newer and a provisioned nightly Rust toolchain are required. Cargo runs offline with the lockfile enforced; tool installation is separate from generation. The [Rustdoc guide](../../docs/documentation-generators.md#configure-a-rustdoc-job) supplies the configuration and executable pins.

The provider uses Cargo’s actual dependency artifacts and exact package versions to choose documentation roots, including renamed dependencies and multiple versions of one crate. It keeps the original input intact and appends hidden documentation imports only to a prepared copy of the library root. The native HTML, original/prepared identities, dependency identities and tool output remain in the existing job result. Broken comment links fail Rustdoc; unresolved local pages or fragments fail target assembly.

For Gazette’s Rust SDK, capture the selected repository and choose `sdk/rust/Cargo.toml` with the `native-local` feature. Select `reference/gazette_sdk` from the output as `rustdoc-html`. The assembled API retains declarations and examples; prepared files, receipts and logs stay with the private job result. The provider’s focused checks run with:

```sh
python3 -B -m unittest discover -s test -p test_rustdoc.py
```

## Generate TypeScript reference

The local `typedoc.json` reads Gazette’s public declarations in `sdk/js`, including Search, Analytics and WebMCP. The lockfile pins TypeDoc, its Markdown plugin and TypeScript. The formatter preserves declaration qualifiers that need special handling, including readonly members, generic constraints and defaults. Other projects select their own declarations and compatible toolchain; this configuration is the Gazette SDK example.

With Node 22.18 or newer, run from this directory to generate that example:

```sh
npm ci --ignore-scripts
npm test
npm run generate
```

The result is `generated/reference/`. For a retained configured job, follow the [runner and formatter setup](../../docs/documentation-generators.md#local-typedoc-markdown-formatter): provision a complete tool copy, select its executable and options explicitly, pass both plugins through repeated `--plugin` arguments with typed machine paths, and import its output as `markdown`. Keep captured source options independent of the machine’s installed paths. TypeDoc JSON can instead go directly through Gazette’s existing `typedoc-json` converter.

## Assemble and inspect the result

Use [documentation modules](../../docs/mintlify.md) to combine generated reference with selected handwritten guides. Check signatures, qualifiers, examples, chapter order and links between guides and symbols in the assembled result. [Directory selections](../../docs/mintlify-formats.md#generated-markdown) derive symbol navigation from the generated tree while the workspace names its chapters. When exports change, those selections include the new pages; explicit page selections still need to cover the complete result without omissions or overlaps.

A completed generator need not run again because later assembly failed. Bind the retained output as an [artifact input](../../docs/documentation-generators.md#reuse-retained-output), check its recorded identity, and assemble it again. Keep the original input, preparation and native output together so the resulting reference can be traced back to the selected code. A successful local assembly and rendering check does not establish hosted publication or publisher approval.
