#!/usr/bin/env python3
"""Create a new, persistent local publication; never replace an existing path."""

import argparse
import json
from pathlib import Path
import shutil


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new publication directory")
    args = parser.parse_args()
    destination = args.destination.expanduser().absolute()
    if destination.exists() or destination.is_symlink():
        parser.error("destination already exists; choose a new directory")
    if not destination.parent.is_dir():
        parser.error("destination's parent directory must already exist")
    destination.mkdir()
    destination = destination.resolve()
    example = Path(__file__).resolve().parent
    for name in ("content", "theme"):
        shutil.copytree(example / name, destination / name)
    def quoted(name):
        return json.dumps(str(destination / name), ensure_ascii=False)

    workspace = f"""[composition]
theme = {{ package = 'theme:field-notes' }}

[[principals]]
id = 'owner:field-notes'
kind = 'person'

[[sites]]
id = 'site:field-notes'
publisher = 'owner:field-notes'
deployable = false

[[brands]]
id = 'brand:field-notes'
name = 'Field Notes'

[[brands.publications]]
site = 'site:field-notes'
public_base_url = 'https://field-notes.example/'
home = 'content:field-notes'
routing = {{ articles = '/', collections = '/', projects = '/', pages = '/' }}
authored = [{{ source = 'source:field-notes', target = 'content' }}]

[[sources]]
id = 'source:field-notes'
location = {{ kind = 'filesystem', root = {quoted('content')} }}

[[sources]]
id = 'source:field-notes-theme'
location = {{ kind = 'filesystem', root = {quoted('theme')} }}

[[theme_packages]]
package = 'theme:field-notes'
source = 'source:field-notes-theme'
manifest = 'theme.toml'
"""
    (destination / "gazette.toml").write_text(workspace, encoding="utf-8")
    request = {
        "format": "gazette-local-publication-set/1",
        "workspace": {"kind": "local", "root": str(destination), "workspace": "gazette.toml"},
        "sites": [{"site": "site:field-notes", "model_output": "model.json"}],
    }
    (destination / "preview-request.json").write_text(
        json.dumps(request, separators=(",", ":")) + "\n", encoding="utf-8"
    )
    editor = destination / ".vscode"
    editor.mkdir()
    (editor / "settings.json").write_text(
        json.dumps({
            "gazette.workspace.configuration": "gazette.toml",
            "gazette.workspace.discovery": True,
            "markdownlint.config": {"MD025": {"front_matter_title": ""}},
        }, indent=2) + "\n", encoding="utf-8"
    )
    print(f"Created {destination}")
    print("Edit content/keep-the-context/index.md; the workspace and output are kept.")
    print("Continue with https://rawthoughts.in/gazette/first-publication/#start-the-publication")


if __name__ == "__main__":
    main()
