# Bind machine-bound programs to the machine you build on

A Gazette build can run native programs bound to one machine: math typesetting and policy checks select their command in the workspace. Use the existing `machine_binding` file to locate each program's executable, working directory and any path arguments on each machine. Gazette resolves these together from the captured configuration before comparing command digests or executing anything.

## Declare the command once

The root workspace selects its usual local binding:

```toml
machine_binding = 'tools/studio.local.toml'
```

A checker declaration can then use named roots:

```toml
[policy_check]
program = 'machine:python/bin/python3'
directory = 'machine:checker'
arguments = [
  '-B',
  { path = 'machine:checker/check.py' },
  'policy',
  '--project',
  { path = 'machine:documents/Projects/Example' },
]
environment = {}
```

The selected `tools/studio.local.toml` supplies those roots alongside its other machine settings:

```toml
[roots]
python = '/opt/example/python'
checker = '/srv/workspaces/checker'
documents = '/srv/documents'
```

A root value can be absolute or relative to the binding file's parent directory. The command never discovers another binding file, searches `PATH`, expands environment variables or uses the process's current directory. Root names and confined suffixes use the existing [source-coordinate rules](workspace-sources.md).

Program, directory and typed argument paths accept three forms: an absolute native path; `machine:name` with an optional confined suffix; or a workspace-relative path, with `.` selecting the workspace root. Workspace-relative paths are relative to the selected root workspace, including when the declaration appears in an included module. They are not relative to the command's working directory. Absolute paths retain their existing spelling, including `/` as a working directory; Gazette does not canonicalize paths or follow links while binding configuration.

String arguments remain literal. For example, `'machine:checker/check.py'` is passed exactly as written; `{ path = 'machine:checker/check.py' }` resolves to the native path. Environment values also remain literal. Use typed path arguments only where the program expects a filesystem path.

## Use the same binding for math

Math keeps its existing retained resource source and entrypoint. Only its program coordinates need machine bindings:

```toml
[math_typesetter]
source = 'source:math-typesetter'
entrypoint = 'render.mjs'
program = 'machine:node/bin/node'
directory = '.'
arguments = []
environment = {}
executable = { bytes = 12345, sha256 = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' }
```

Add `node` under the existing binding file's `[roots]` and replace the illustrative executable length and digest with the selected executable's actual values. The typesetter still requires a pin and receives its entrypoint from captured resources. A checker pin is optional for ordinary local execution and required for independently bound candidate/output checks; see [configured policy checks](policy-checks.md).

## Capture, compare and execute

Pure workspace compilation validates declarations without resolving native roots or reading files. Native workspace loading captures the selected binding with the other configuration files. `configuration::ResolvedWorkspace::math_command()` and `policy_command()` expose the native commands resolved from those retained bytes. Single-site and multi-site acquired inputs expose that same configuration through `configuration()`.

An absent command returns `None`. A selected command with an unknown root, no required workspace context, a non-UTF-8 native path or excessive expanded text returns `runtime_command_binding` when requested. It cannot become an unconfigured checker or a successful check. An unused command's binding error does not prevent inspecting the workspace or using another independently configured command. Detached TOML can still use absolute native commands, but has no machine or workspace-relative context.

Both declarations and expanded native commands enforce the existing limits: at most 256 arguments, 64 environment entries and 64 KiB across program, directory, argument and environment text. Native execution additionally requires absolute program and directory paths and explicit execution permission. Binding does not establish that a file exists, that a program is trusted or that its effects are confined.

The independent policy `command_sha256` is the SHA-256 of compact JSON for the **bound `gazette_core::process::Command`**, including the absolute program, directory, resolved path arguments, literal arguments, environment and executable pin. Do not hash the portable declaration. Relocating a program may preserve the logical workspace declaration while changing this native command digest; independent expectations must select the new command deliberately. The captured machine file remains configuration evidence. An already acquired build never reopens a changed binding file to replace its command.

Executable pins still cover only the executable file. Scripts, imports, retained policy sources and other inputs keep their own checks. A bound command and successful invocation do not supply publication approval.
