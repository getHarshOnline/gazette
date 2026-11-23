# Gazette JavaScript and TypeScript SDK

Call Gazette directly from a Node application. Your code supplies configuration or publication inputs; the native addon uses the same Rust engine as the CLI, agent server and editor. It returns native JSON and, for generated files, binary buffers.

The [browser entrypoints](docs/browser.md) provide search, analytics and public reader tools without loading the addon. If you want to use Gazette rather than embed it, start with [your first publication](https://rawthoughts.in/gazette/first-publication/).

## Build and load the local addon

The package is currently private source, not a published npm package with a prebuilt binary. You need Node 22 or later and the [Gazette source-build prerequisites](../../docs/install.md#what-a-build-needs).

:::::steps

::::step "Build the addon"

From the Gazette repository root:

```sh
cargo build --locked -p gazette-node
```

::::

::::step "Package it for Node"

Cargo produces `target/debug/libgazette_node.dylib` on macOS or `target/debug/libgazette_node.so` on Linux when using its default target directory. If you selected a different Cargo target directory or target triple, use that build's actual library path. The JavaScript loader requires a `.node` file at an absolute path. Copy the library to a new `.node` file using the repository helper:

:::code-group

```sh macOS
node sdk/js/tools/package-addon.mjs /absolute/build/libgazette_node.dylib /absolute/local/gazette.node
```

```sh Linux
node sdk/js/tools/package-addon.mjs /absolute/build/libgazette_node.so /absolute/local/gazette.node
```

:::

Use the absolute paths to your build and a new destination file.

The helper copies without rewriting the library and checks the input/output SHA-256. It refuses an existing destination. If it fails after creating a file, inspect that file before choosing another destination; it never removes your files automatically. The SDK does not install an addon or select a Node runtime.

::::

:::::

## Check a configuration and locate a field

Suppose your application lets an author edit publication configuration. It needs to reject an unknown publisher and point to the exact declaration being inspected. This example supplies a complete small workspace, validates it, locates a field in the original bytes, then demonstrates a semantic error. These calls inspect only the supplied text; they do not open source directories, render a page or publish.

Save the following as `sdk-example.mjs` in the Gazette repository root. The relative import uses this checkout's loader; it does not install a package.

```javascript
import { loadGazette } from './sdk/js/index.mjs';

const gazette = loadGazette(process.argv[2]);
const sourceToml = `[[principals]]
id = 'owner:example'
kind = 'person'

[[sites]]
id = 'site:example'
publisher = 'owner:example'
deployable = false
`;
const bytes = new TextEncoder().encode(sourceToml);
const compilationJson = gazette.checkWorkspace(bytes);
console.log('Workspace valid');

const origin = JSON.parse(gazette.fieldOrigin(bytes, JSON.stringify({
  kind: 'site', identity: 'site:example', field: ['deployable'],
})));
console.log('Deployable literal:', new TextDecoder().decode(
  bytes.subarray(origin.span.start, origin.span.end),
));

try {
  gazette.checkWorkspace(new TextEncoder().encode(
    sourceToml.replace("publisher = 'owner:example'", "publisher = 'owner:missing'"),
  ));
  throw new Error('Expected invalid publisher to fail');
} catch (error) {
  const failure = JSON.parse(error.message);
  console.log('Invalid publisher:', failure.code);
}
```

Run it with your packaged addon:

```sh
node sdk-example.mjs /absolute/local/gazette.node
```

The output is:

```text
Workspace valid
Deployable literal: false
Invalid publisher: workspace_invalid
```

`compilationJson` is the complete resolved workspace as a JSON string, including its source digest. A failed compilation throws a native failure instead of returning a partially resolved workspace. The field query identifies a site and an explicit field; its half-open `span` addresses UTF-8 bytes in the supplied text. Decode those bytes as shown rather than using those offsets to slice a JavaScript string, whose positions use UTF-16.

The example parses the small origin/failure records, whose offsets fit safely within its supplied input. Native responses can contain integers beyond JavaScript precision. Preserve the original JSON and use a lossless reader where those values matter. Configuration diagnostics can contain your supplied identifiers; keep private inputs and their diagnostics out of public logs.

If loading fails, check that the argument is an absolute `.node` path, that the library was built for your operating system and architecture, and that the loader and addon come from the same checkout. An older addon can lack an operation required by the current loader. Invalid TOML reports `toml_invalid`; an unresolved publisher reports `workspace_invalid`. Correct the input and call the pure checker again. For effectful rendering/preparation calls, follow their own failure and recovery contracts rather than applying that retry pattern.

## Continue with the operation you need

| Integration task | Reference |
| --- | --- |
| Inspect configuration, field origins, Markdown or page packages | [Node API reference](docs/native-api.md) |
| Render a selected publication and receive HTML/assets as buffers | [Rendering and file delivery](docs/native-api.md#private-local-publication-rendering) |
| Assemble developer manuals and generated references | [Documentation assembly](docs/native-api.md#documentation-repository-assembly) |
| Review, qualify or prepare publication output | [Node operation contracts](docs/native-api.md#private-publication-review) |
| Compare served output or inspect immutable release history | [Delivery](docs/native-api.md#observe-served-output), [history](docs/native-api.md#immutable-release-history-inspection) |
| Integrate browser search, analytics or WebMCP | [Browser SDK integration](docs/browser.md) |

The [TypeScript declarations](index.d.ts) describe the ESM exports, readonly methods, options and return types. `NativeJson` is a string boundary, not a second workspace schema. The detailed reference explains synchronous calls, independent input/result limits, permissions and native failures. The SDK currently has no standalone store administration, release dispatch, persistent parsed handles, Wasm interface or distributed native-binary installation.

## Run the SDK tests

The native-addon tests compare what the addon returns with an independent Rust SDK read of the same input, so a passing run describes the addon and Rust bytes it was built from; rebuild before running them after any change. The addon crate denies handwritten unsafe code; napi-rs's generated registration can emit its own local allowance, and the rest of the workspace keeps its unsafe-code prohibition.

Build the addon and the Rust reference examples, then package the addon as described above:

```sh
cargo build --locked -p gazette-node --lib --examples
node sdk/js/tools/package-addon.mjs target/debug/libgazette_node.dylib /absolute/local/gazette.node
```

Each test takes the packaged addon's absolute path. `publication.mjs`, `inspection_tree.mjs`, `history.mjs`, `documentation.mjs` and `preparation.mjs` also take the matching Rust example from `target/debug/examples/` (`publication_reference`, `inspection_reference`, `history_reference`, `documentation_reference` and `preparation_reference`):

```sh
node sdk/js/tests/history.mjs /absolute/local/gazette.node target/debug/examples/history_reference
```

`native.mjs` and `typescript.mjs` read the `workspace_reference` example's JSON on standard input. `typescript.mjs` also takes the absolute path of a TypeScript compiler script. It copies only the declared package payload into a fresh isolated consumer and compiles it with strict `NodeNext` resolution, `skipLibCheck: false`, no ambient types and `noEmitOnError`; the consumer then runs the supplied-source operations and compares complete values with the Rust reference, and compile-time negative cases check wrong types, unsupported methods and readonly methods. Withholding the declarations must make compilation fail.

```sh
target/debug/examples/workspace_reference > /absolute/local/workspace-reference.json
node sdk/js/tests/typescript.mjs /absolute/typescript/bin/tsc /absolute/local/gazette.node < /absolute/local/workspace-reference.json
```

`delivery.mjs` and `qualification.mjs` run inside native fixtures that own the live HTTP server, retained tree and reports. Run them through Cargo with `GAZETTE_TEST_NODE` set to an absolute Node executable and `GAZETTE_TEST_ADDON` to the packaged addon:

```sh
cargo test --locked -p gazette-launcher --test delivery javascript:: -- --ignored --test-threads=1
cargo test --locked -p gazette-launcher --test publication_render js_qualification:: -- --ignored --test-threads=1
```

The browser modules need no addon: `node --test sdk/js/tests/search.mjs sdk/js/tests/search-browse.mjs sdk/js/tests/analytics.mjs sdk/js/tests/webmcp.mjs`, with `GAZETTE_TEST_TYPESCRIPT` naming a `typescript.js` for the declaration checks. The portable workflow, `.github/workflows/portable.yml`, runs all of these.
