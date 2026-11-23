# Direct Gazette quickstart

This disposable example proves that the standalone `gazette` command can build a one-site workspace from exact pinned Git inputs. It creates a minimal Theme and authored page in a temporary repository, compiles the shared model, retains fresh local HTML and verifies the result.

From the Gazette repository root:

```sh
sh examples/quickstart/run.sh
```

The default route invokes the selected workspace binary through Cargo, so it honors `CARGO_TARGET_DIR` and Cargo configuration without reconstructing a platform-specific artifact path or creating another target policy.

Pass an existing Gazette executable as the first argument to skip the build:

```sh
sh examples/quickstart/run.sh /absolute/path/to/gazette
```

The example deliberately uses no Pale runtime, database, network, provider account, push or deployment. Its temporary output is private and draft-inclusive. Passing the example proves direct local compilation; it does not prove public admission, technical certification or publisher authority.

The script removes its disposable directory on ordinary exit and handled signals. A host crash can leave that private temporary directory behind, but no remote, store, authorization or release effect needs recovery. Inspect or remove the residue explicitly; rerunning the quickstart creates a fresh directory and never adopts, resumes or overwrites the old one.
