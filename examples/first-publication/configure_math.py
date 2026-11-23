#!/usr/bin/env python3
"""Prepare explicit local math settings for an existing example publication."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gazette-source", type=Path, required=True)
    parser.add_argument("--request", type=Path, required=True)
    parser.add_argument("--node", type=Path, required=True)
    args = parser.parse_args()
    root = Path.cwd().resolve()
    renderer = args.gazette_source.resolve(strict=True) / "adapters" / "math"
    node = args.node.resolve(strict=True)
    for relative in ("render.mjs", "vendor/katex-0.18.4/katex.mjs", "vendor/KATEX-LICENSE"):
        if not (renderer / relative).is_file():
            parser.error(f"missing maintained math input: {relative}")
    version = subprocess.check_output([str(node), "--version"], text=True).strip()
    if int(version.removeprefix("v").split(".")[0]) < 20:
        parser.error("the maintained renderer requires Node 20 or later")
    request = json.loads(args.request.resolve(strict=True).read_text(encoding="utf-8"))
    workspace = request["workspace"]
    if workspace["kind"] != "local" or Path(workspace["root"]).resolve() != root:
        parser.error("run this from the publication named by --request")
    settings_path = root / "math-runtime.toml"
    request_path = root / "preview-math-request.json"
    for destination in (settings_path, request_path):
        if destination.exists() or destination.is_symlink():
            parser.error(f"destination already exists; keep it or choose another publication: {destination}")
    node_bytes = node.read_bytes()
    quote = lambda value: json.dumps(str(value), ensure_ascii=False)
    settings = f"""[math_typesetter]
source = 'source:field-notes-math'
entrypoint = 'render.mjs'
program = {quote(node)}
executable = {{ bytes = {len(node_bytes)}, sha256 = '{hashlib.sha256(node_bytes).hexdigest()}' }}
arguments = []
directory = {quote(root)}
environment = {{}}

[[sources]]
id = 'source:field-notes-math'
location = {{ kind = 'filesystem', root = {quote(renderer)} }}
"""
    request["math_generation"] = {
        "allow_execute": True,
        "max_input_bytes": 65536,
        "timeout_ms": 10000,
        "formulas": 32,
        "output_bytes": 1048576,
    }
    with settings_path.open("x", encoding="utf-8") as output:
        output.write(settings)
    with request_path.open("x", encoding="utf-8") as output:
        output.write(json.dumps(request, separators=(",", ":")) + "\n")
    print(f"Created {settings_path} and {request_path}")
    print(f"Selected {version}; retained renderer: {renderer}")


if __name__ == "__main__":
    main()
