#!/usr/bin/env python3
"""Prepare Cargo's selected library for the configured Gazette generator."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
from urllib.parse import urlsplit

VERSION = "gazette-rustdoc-provider/2"
JSON_BYTES = 64 << 20
CRATES_IO = {"registry+https://github.com/rust-lang/crates.io-index", "sparse+https://index.crates.io/"}


class Refusal(Exception):
    """A selected input or native result cannot support complete reference generation."""


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            value.update(block)
    return value.hexdigest()


def tree(root):
    """Inventory regular files without following links, including empty directories."""
    if root.is_symlink() or not root.is_dir():
        raise Refusal("capture root must be a real directory")
    result = {}
    for path in sorted(root.rglob("*")):
        mode = path.lstat().st_mode
        name = path.relative_to(root).as_posix()
        if stat.S_ISDIR(mode):
            result[name] = {"kind": "directory"}
        elif stat.S_ISREG(mode):
            result[name] = {"kind": "file", "bytes": path.stat().st_size, "sha256": digest(path)}
        else:
            raise Refusal(f"source contains a link or special file: {name}")
    return result


def identity(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def contained(root, value):
    root = root.resolve(strict=True)
    path = Path(value)
    if not path.is_absolute():
        path = root / path
    path = path.resolve(strict=True)
    if not path.is_relative_to(root):
        raise Refusal(f"selected path is outside the captured tree: {value}")
    return path


def read_json(path):
    if path.stat().st_size > JSON_BYTES:
        raise Refusal(f"JSON exceeds {JSON_BYTES} bytes: {path.name}")
    return json.loads(path.read_text())


def tool(path):
    if not isinstance(path, str) or not path:
        raise Refusal("cargo, rustc and rustdoc must name explicit installed executable paths")
    path = Path(path)
    if not path.is_absolute() or not path.is_file():
        raise Refusal("cargo, rustc and rustdoc must name explicit installed executable paths")
    path = path.resolve(strict=True)
    return {"path": str(path), "bytes": path.stat().st_size, "sha256": digest(path)}


def selected_tools(config, args):
    """Combine explicit selections, refusing conflicting JSON or CLI declarations."""
    selected = {}
    for name in ("cargo", "rustc", "rustdoc"):
        values = ([config[name]] if name in config else []) + (getattr(args, name, None) or [])
        if not values or any(not isinstance(value, str) or not value for value in values):
            raise Refusal(f"{name} requires an explicit executable path in configuration or --{name}")
        if len(set(values)) != 1:
            raise Refusal(f"conflicting {name} executable paths in configuration or command arguments")
        selected[name] = tool(values[0])
    return selected


def command(arguments, cwd, environment, output, name, evidence):
    stdout = output / f"{name}.stdout"
    stderr = output / f"{name}.stderr"
    record = {"arguments": list(map(str, arguments)), "directory": str(cwd), "exit": None}
    evidence["invocations"].append(record)
    with stdout.open("xb") as out, stderr.open("xb") as err:
        process = subprocess.run(record["arguments"], cwd=cwd, env=environment, stdout=out, stderr=err, check=False)
    record.update(exit=process.returncode, stdout_sha256=digest(stdout), stderr_sha256=digest(stderr))
    if stdout.stat().st_size > JSON_BYTES or stderr.stat().st_size > JSON_BYTES:
        raise Refusal(f"{name} output exceeds the capture limit")
    if process.returncode:
        with stderr.open("rb") as source:
            source.seek(max(0, stderr.stat().st_size - 8192))
            print(source.read().decode("utf-8", errors="replace"), file=sys.stderr)
        raise Refusal(f"{name} failed with exit {process.returncode}; inspect its retained output")
    return stdout


def library(metadata, manifest):
    packages = [item for item in metadata["packages"] if Path(item["manifest_path"]) == manifest]
    if len(packages) != 1:
        raise Refusal("manifest must select exactly one Cargo package, not a workspace-only manifest")
    package = packages[0]
    targets = [item for item in package["targets"] if any(kind in {"lib", "rlib", "proc-macro"} for kind in item["kind"])]
    if len(targets) != 1:
        raise Refusal("selected package must have exactly one library target")
    return package, targets[0]


def roots(metadata, overrides):
    """Resolve explicit roots by complete Cargo identity; reject ambiguous name/version keys."""
    packages = {item["id"]: item for item in metadata["packages"]}
    result = {}
    for key, url in overrides.items():
        candidates = [item for item in packages.values() if key in {item["id"], f'{item["name"]}@{item["version"]}'}]
        parsed = urlsplit(url)
        if len(candidates) != 1 or parsed.scheme != "https" or not parsed.netloc or parsed.query or parsed.fragment or parsed.username or parsed.password or not url.endswith("/"):
            raise Refusal(f"dependency root needs one exact package and an absolute HTTPS directory: {key}")
        result[candidates[0]["id"]] = url
    for item in packages.values():
        if item.get("source") in CRATES_IO:
            result.setdefault(item["id"], f'https://docs.rs/{item["name"]}/{item["version"]}/')
    return result


def dependencies(metadata, messages, target, selected, source_hash, overrides):
    """Use Cargo's artifact identities, never a glob over previously installed versions."""
    packages = {item["id"]: item for item in metadata["packages"]}
    urls = roots(metadata, overrides)
    selected_artifacts = {}
    for message in messages:
        if message.get("reason") != "compiler-artifact" or message["package_id"] == selected:
            continue
        package = packages.get(message["package_id"])
        if package is None:
            raise Refusal("Cargo artifact package is absent from the resolved metadata")
        if package["id"] not in urls:
            continue
        paths = message["filenames"]
        candidates = [path for path in paths if path.endswith(".rmeta")]
        if not candidates:
            candidates = [path for path in paths if path.endswith(".rlib") or ("proc-macro" in message["target"]["kind"] and Path(path).suffix in {".so", ".dylib", ".dll"})]
        if not candidates:
            continue
        if len(candidates) != 1:
            raise Refusal("Cargo reported ambiguous library artifacts")
        artifact = contained(target, candidates[0])
        if not artifact.is_file():
            raise Refusal("Cargo library artifact is not a regular file")
        artifact_hash = digest(artifact)
        # Name and order artifacts by Cargo's identity and file name, which a locked
        # build repeats; metadata bytes need not be reproducible between builds, so
        # their digest stays evidence and never shapes the prepared source.
        key = (package["id"], message["target"]["name"], artifact.name)
        selected_artifacts[key] = {
            "package_id": package["id"], "name": package["name"], "version": package["version"],
            "crate": message["target"]["name"], "artifact": str(artifact), "artifact_sha256": artifact_hash,
            "root": urls[package["id"]],
        }
    result = []
    for key, artifact in sorted(selected_artifacts.items()):
        # The source-derived namespace cannot collide with a predictable existing crate alias.
        suffix = identity(key)[:24]
        artifact["alias"] = f"gazette_doc_{source_hash[:24]}_{suffix}"
        result.append(artifact)
    return result


def prepare(source, artifacts):
    text = source.decode("utf-8")
    imports = []
    for item in artifacts:
        alias = item["alias"]
        if re.search(rf"\b{re.escape(alias)}\b", text):
            raise Refusal("documentation dependency alias already occurs in the source")
        imports.append(f"#[cfg(doc)]\n#[doc(hidden)]\n#[allow(unused_extern_crates)]\nextern crate {alias};\n")
    return source + ("\n" + "".join(imports)).encode() if imports else source


def generate(args):
    source = Path(args.input).resolve(strict=True)
    output = Path(args.output).resolve(strict=True)
    if not source.is_dir() or not output.is_dir() or source.is_relative_to(output) or output.is_relative_to(source) or any(output.iterdir()):
        raise Refusal("generation needs separate source and empty output directories")
    config = read_json(contained(source, args.config))
    allowed = {"manifest", "features", "no_default_features", "cargo", "rustc", "rustdoc", "dependency_roots"}
    if set(config) - allowed or "manifest" not in config:
        raise Refusal("unknown or missing Rustdoc configuration field")
    features = config.get("features", [])
    if not isinstance(features, list) or not all(isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_/-]+", value) for value in features):
        raise Refusal("features must be explicit Cargo feature names")
    if type(config.get("no_default_features", False)) is not bool or not isinstance(config.get("dependency_roots", {}), dict):
        raise Refusal("invalid feature or dependency-root configuration")
    tools = selected_tools(config, args)
    original = tree(source)
    provider = Path(__file__).resolve()
    provider_hash = digest(provider)
    evidence = {"format": VERSION, "status": "incomplete", "provider_sha256": provider_hash, "original": {"path": str(source), "identity": identity(original), "files": original}, "tools": tools, "invocations": []}
    prepared = output / "prepared"
    target = output / ".cargo-work"
    try:
        shutil.copytree(source, prepared)
        manifest = contained(prepared, config["manifest"])
        original_manifest = contained(source, config["manifest"])
        if manifest.relative_to(prepared) != original_manifest.relative_to(source):
            raise Refusal("manifest does not name the same original and prepared source")
        environment = dict(os.environ, RUSTC=tools["rustc"]["path"], RUSTDOC=tools["rustdoc"]["path"], CARGO_TARGET_DIR=str(target))
        for name in ("cargo", "rustc", "rustdoc"):
            command([tools[name]["path"], "--version"], prepared, environment, output, name + "-version", evidence)
        common = ["--offline", "--locked", "--manifest-path", str(manifest)]
        if features:
            common += ["--features", ",".join(features)]
        if config.get("no_default_features", False):
            common += ["--no-default-features"]
        cargo = tools["cargo"]["path"]
        metadata = read_json(command([cargo, "metadata", *common, "--format-version", "1"], prepared, environment, output, "metadata", evidence))
        if Path(metadata["workspace_root"]).resolve() != prepared and not Path(metadata["workspace_root"]).resolve().is_relative_to(prepared):
            raise Refusal("Cargo workspace escapes the captured input")
        for package in metadata["packages"]:
            if package.get("source") is None:
                contained(prepared, package["manifest_path"])
        package, library_target = library(metadata, manifest)
        root = contained(prepared, library_target["src_path"])
        before = root.read_bytes()
        if tree(prepared) != original:
            raise Refusal("Cargo metadata changed the prepared source")
        checked = command([cargo, "check", *common, "--package", package["id"], "--lib", "--message-format=json"], prepared, environment, output, "check", evidence)
        messages = [json.loads(line) for line in checked.read_text().splitlines() if line]
        if not any(item.get("reason") == "build-finished" and item.get("success") is True for item in messages):
            raise Refusal("Cargo did not report a complete successful build")
        artifacts = dependencies(metadata, messages, target.resolve(strict=True), package["id"], hashlib.sha256(before).hexdigest(), config.get("dependency_roots", {}))
        root.write_bytes(prepare(before, artifacts))
        prepared_tree = tree(prepared)
        expected = dict(original)
        relative = root.relative_to(prepared).as_posix()
        expected[relative] = {"kind": "file", "bytes": root.stat().st_size, "sha256": digest(root)}
        if prepared_tree != expected:
            raise Refusal("preparation changed a file other than the selected library root")
        evidence["preparation"] = {"path": str(prepared), "identity": identity(prepared_tree), "crate_root": relative, "original_sha256": hashlib.sha256(before).hexdigest(), "prepared_sha256": digest(root), "dependencies": artifacts}
        flags = ["-D", "warnings", "--cap-lints", "deny", "-Z", "unstable-options", "--extern-html-root-takes-precedence"]
        for artifact in artifacts:
            flags += ["--extern", f'{artifact["alias"]}={artifact["artifact"]}', "--extern-html-root-url", f'{artifact["alias"]}={artifact["root"]}']
        command([cargo, "rustdoc", *common, "--package", package["id"], "--lib", "--", *flags], prepared, environment, output, "rustdoc", evidence)
        if tree(source) != original or tree(prepared) != prepared_tree:
            raise Refusal("the original or prepared source changed during generation")
        for name, observed in tools.items():
            if tool(observed["path"]) != observed:
                raise Refusal(f"{name} changed during generation")
        if digest(provider) != provider_hash:
            raise Refusal("Rustdoc provider changed during generation")
        native = contained(output, target / "doc")
        tree(native)
        shutil.copytree(native, output / "reference")
        # Cargo's disposable products are not reference or source evidence. Only
        # this job's newly created directory is removed, after successful capture.
        shutil.rmtree(target)
        evidence["status"] = "generated"
        evidence["reference_identity"] = identity(tree(output / "reference"))
    except Exception as error:
        evidence["failure"] = str(error)
        raise
    finally:
        (output / "preparation.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps({"status": "generated", "preparation": str(output / "preparation.json"), "reference": str(output / "reference")}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", action="version", version=VERSION)
    parser.add_argument("--input", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--config", required=True, help="configuration path relative to the captured input")
    for name in ("cargo", "rustc", "rustdoc"):
        parser.add_argument(f"--{name}", action="append", help=f"explicit installed {name} executable; must agree with any configuration selection")
    try:
        generate(parser.parse_args())
    except (Refusal, OSError, ValueError, KeyError, TypeError) as error:
        print(f"Rustdoc generation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
