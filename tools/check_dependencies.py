#!/usr/bin/env python3
r"""Check Gazette's locked dependencies against current advisory services.

Run with Python 3.11 or later and already provisioned scanners. Exact scanner
versions, service endpoints, selected dependency locks and expected digests
come from tools/distribution-evidence.toml. Provision the scanners locally,
using tools/security/pip-audit-requirements.txt for the Python scanner closure.

From the repository root, supply absolute scanner paths and an output directory
outside the source checkout:

    python3 tools/check_dependencies.py \
        --output-dir /temporary/gazette-dependency-check \
        --cargo-audit /tools/cargo-audit \
        --npm /tools/npm \
        --pip-audit /tools/pip-audit \
        --govulncheck /tools/govulncheck

The configured checks cover the Rust lock, both npm locks, the hash-locked
Sphinx requirements and the readonly Go provider module graph. cargo-audit
fetches RustSec into a temporary database; npm, pip-audit and govulncheck query
the configured advisory services. Network access is required. Scanners may
use their own caches; read-only describes the selected dependency inputs,
not an isolated process or a promise of no host-side writes. No dependency
update, fix or upgrade operation is requested. Input digests and scanner
executable identities are checked before and after the scans; exact normalized
versions must match, including the Go toolchain reported by govulncheck.

The output contains receipt.json, raw JSON reports and stderr for each scanner.
The receipt binds lock digests, scanner versions and executable digests, report
digests, scan time and the strongest advisory-data identity available: RustSec's
Git database identity and Go's db_last_modified observation. npm and PyPI supply
no stable database revision here, so their service endpoints, observation time
and response digests identify those observations instead.

A completed check prints the receipt path. Findings or unsuccessful scanner
results retain that receipt and exit 1; invalid inputs, changed executables or
unreadable reports fail before a complete receipt is available. A green result
means only that these scanners reported no known finding for those exact inputs
against the observed services. It proves neither absence of vulnerabilities nor
native-library completeness, reachability, licence compatibility, legal approval
or publication permission. The receipt is volatile evidence outside deterministic
build and publication artifact identity. Review the findings for the selected
release inputs.

Provider choices, limits and maintainer checks live in
docs/maintain/dependencies.md, relative to this repository's root.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import subprocess
import tempfile
import tomllib
from pathlib import Path
from typing import Any


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def confined(root: Path, raw: str) -> Path:
    selected = root / raw
    if selected.is_symlink():
        raise ValueError(f"dependency input is not a confined regular file: {raw}")
    candidate = selected.resolve(strict=True)
    if not candidate.is_file() or not candidate.is_relative_to(root):
        raise ValueError(f"dependency input is not a confined regular file: {raw}")
    return candidate


def locked_inputs(root: Path, config: dict[str, Any]) -> list[dict[str, str]]:
    selected = [(config["cargo"]["lock"], config["cargo"]["sha256"])]
    selected.extend(
        (row["path"], row["sha256"]) for row in config.get("scanner_lock", [])
    )
    selected.extend((row["path"], row["sha256"]) for row in config["node_lock"])
    selected.extend((row["path"], row["sha256"]) for row in config["python_lock"])
    for row in config["go_lock"]:
        selected.extend(
            [(row["module"], row["module_sha256"]), (row["sum"], row["sum_sha256"])]
        )
    evidence = []
    for raw, expected in selected:
        path = confined(root, raw)
        actual = digest(path)
        if actual != expected:
            raise ValueError(f"dependency input drift: {raw}: expected={expected} actual={actual}")
        evidence.append({"path": raw, "sha256": actual})
    return evidence


def run(
    command: list[str], *, cwd: Path, environment: dict[str, str] | None = None
) -> dict[str, Any]:
    completed = subprocess.run(
        command,
        cwd=cwd,
        env={**os.environ, **(environment or {})},
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    return {
        "command": command,
        "exit_code": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def write_report(output: Path, name: str, result: dict[str, Any]) -> dict[str, Any]:
    body = result["stdout"].encode()
    path = output / f"{name}.json"
    path.write_bytes(body)
    stderr = result["stderr"].encode()
    stderr_path = output / f"{name}.stderr.txt"
    stderr_path.write_bytes(stderr)
    return {
        "exit_code": result["exit_code"],
        "report": path.name,
        "report_sha256": hashlib.sha256(body).hexdigest(),
        "stderr": stderr_path.name,
        "stderr_sha256": hashlib.sha256(stderr).hexdigest(),
    }


def require_exact_version(label: str, observed: str, expected: str) -> None:
    if observed != expected:
        raise ValueError(f"{label} version mismatch: expected {expected!r}, observed {observed!r}")


def require_named_version(label: str, observed: str, prefix: str, expected: str) -> None:
    matches = [
        line.removeprefix(prefix).strip()
        for line in observed.splitlines()
        if line.startswith(prefix)
    ]
    if matches != [expected]:
        raise ValueError(f"{label} version mismatch: expected {expected!r}, observed {observed!r}")


def version_text(result: dict[str, Any]) -> str:
    return (result["stdout"] + result["stderr"]).strip()


def executable_identity(path: Path) -> dict[str, Any]:
    resolved = path.resolve(strict=True)
    if not resolved.is_file():
        raise ValueError(f"scanner executable is not a regular file: {path}")
    return {"sha256": digest(resolved), "bytes": resolved.stat().st_size}


def json_value(result: dict[str, Any], label: str) -> Any:
    try:
        return json.loads(result["stdout"])
    except json.JSONDecodeError as error:
        raise ValueError(f"{label} did not emit JSON: {error}") from error


def json_stream(text: str, label: str) -> list[Any]:
    decoder = json.JSONDecoder()
    values = []
    cursor = 0
    while cursor < len(text):
        while cursor < len(text) and text[cursor].isspace():
            cursor += 1
        if cursor == len(text):
            break
        try:
            value, cursor = decoder.raw_decode(text, cursor)
        except json.JSONDecodeError as error:
            raise ValueError(f"{label} did not emit a JSON stream: {error}") from error
        values.append(value)
    return values


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--source-root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--config", type=Path, default=Path("tools/distribution-evidence.toml"))
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--cargo-audit", type=Path, required=True)
    parser.add_argument("--pip-audit", type=Path, required=True)
    parser.add_argument("--govulncheck", type=Path, required=True)
    parser.add_argument("--npm", type=Path, required=True)
    args = parser.parse_args()

    root = args.source_root.resolve(strict=True)
    output = args.output_dir.resolve()
    if output == root or output.is_relative_to(root):
        raise SystemExit("qualification output must remain outside the source tree")
    output.mkdir(parents=True, exist_ok=True)
    config_path = args.config if args.config.is_absolute() else root / args.config
    config = tomllib.loads(config_path.read_text(encoding="utf-8"))
    security = config["security_qualification"]
    before = locked_inputs(root, config)
    scanner_paths = {
        "cargo_audit": args.cargo_audit,
        "npm": args.npm,
        "pip_audit": args.pip_audit,
        "govulncheck": args.govulncheck,
    }
    scanner_identities = {
        label: executable_identity(path) for label, path in scanner_paths.items()
    }

    versions = {
        "cargo_audit": run([str(args.cargo_audit), "--version"], cwd=root),
        "npm": run([str(args.npm), "--version"], cwd=root),
        "pip_audit": run([str(args.pip_audit), "--version"], cwd=root),
        "govulncheck": run([str(args.govulncheck), "-version"], cwd=root),
    }
    for label, result in versions.items():
        if result["exit_code"] != 0:
            raise SystemExit(f"cannot identify {label}: {result['stderr'].strip()}")
    require_exact_version(
        "cargo-audit",
        version_text(versions["cargo_audit"]),
        f"cargo-audit {security['cargo_audit_version']}",
    )
    require_exact_version("npm", version_text(versions["npm"]), security["npm_version"])
    require_exact_version(
        "pip-audit",
        version_text(versions["pip_audit"]),
        f"pip-audit {security['pip_audit_version']}",
    )
    require_named_version(
        "govulncheck",
        version_text(versions["govulncheck"]),
        "Scanner: govulncheck@",
        security["govulncheck_version"],
    )
    require_named_version(
        "Go toolchain",
        version_text(versions["govulncheck"]),
        "Go: ",
        security["go_toolchain_version"],
    )

    with tempfile.TemporaryDirectory(prefix="gazette-rustsec-") as database:
        rust = run(
            [
                str(args.cargo_audit), "audit", "--db", database,
                "--url", security["rustsec_repository"],
                "--file", str(root / config["cargo"]["lock"]),
                "--no-yanked", "--deny", "warnings", "--json",
            ],
            cwd=root,
        )
    rust_report = json_value(rust, "cargo-audit")

    npm_results = []
    for index, selected in enumerate(config["node_lock"]):
        lock = root / selected["path"]
        result = run(
            [
                str(args.npm), "audit", "--package-lock-only", "--json",
                "--audit-level=low", "--registry", security["npm_registry"],
            ],
            cwd=lock.parent,
        )
        report = json_value(result, f"npm audit {selected['path']}")
        npm_results.append((index, selected, result, report))

    python_selected = config["python_lock"]
    if len(python_selected) != 1:
        raise ValueError("security qualification expects one Python provider lock")
    python = run(
        [
            str(args.pip_audit), "--requirement", str(root / python_selected[0]["path"]),
            "--require-hashes", "--disable-pip", "--strict", "--format", "json",
            "--progress-spinner", "off", "--desc", "off", "--aliases", "on",
            "--vulnerability-service", security["python_advisory_service"],
        ],
        cwd=root,
    )
    python_report = json_value(python, "pip-audit")

    go_selected = config["go_lock"]
    if len(go_selected) != 1:
        raise ValueError("security qualification expects one Go provider graph")
    go_root = (root / go_selected[0]["module"]).parent
    go = run(
        [
            str(args.govulncheck), "-json", "-scan=module",
            f"-db={security['go_vulnerability_database']}",
        ],
        cwd=go_root,
        environment={"GOTOOLCHAIN": "local", "GOFLAGS": "-mod=readonly"},
    )
    go_messages = json_stream(go["stdout"], "govulncheck")
    go_configuration = next(
        (item["config"] for item in go_messages if "config" in item), None
    )
    if not go_configuration:
        raise ValueError("govulncheck report lacks its configuration observation")
    if go_configuration.get("go_version") != security["go_toolchain_version"]:
        raise ValueError(
            "govulncheck used an unexpected Go toolchain: "
            f"expected={security['go_toolchain_version']} "
            f"actual={go_configuration.get('go_version')}"
        )

    after = locked_inputs(root, config)
    if before != after:
        raise ValueError("dependency inputs changed during read-only qualification")
    if scanner_identities != {
        label: executable_identity(path) for label, path in scanner_paths.items()
    }:
        raise ValueError("scanner executable changed during qualification")

    reports: dict[str, Any] = {"rust": write_report(output, "rust", rust)}
    for index, selected, result, _ in npm_results:
        reports[f"npm_{index}"] = {
            **write_report(output, f"npm-{index}", result),
            "input": selected["path"],
        }
    reports["python"] = write_report(output, "python", python)
    reports["go"] = write_report(output, "go", go)

    rust_vulnerabilities = rust_report.get("vulnerabilities", {}).get("count", 0)
    npm_vulnerabilities = sum(
        item.get("metadata", {}).get("vulnerabilities", {}).get("total", 0)
        for _, _, _, item in npm_results
    )
    python_vulnerabilities = sum(
        len(item.get("vulns", [])) for item in python_report.get("dependencies", [])
    )
    go_findings = sum(1 for item in go_messages if "finding" in item)
    successful = (
        rust["exit_code"] == 0 and rust_vulnerabilities == 0
        and all(result["exit_code"] == 0 for _, _, result, _ in npm_results)
        and npm_vulnerabilities == 0
        and python["exit_code"] == 0 and python_vulnerabilities == 0
        and go["exit_code"] == 0 and go_findings == 0
    )
    receipt = {
        "schema": "gazette-dependency-security-qualification/1",
        "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "source_revision": os.environ.get("GITHUB_SHA"),
        "boundary": (
            "volatile advisory observation; excluded from deterministic build "
            "and artifact identity"
        ),
        "inputs": before,
        "scanners": {
            label: {
                "version": version_text(result),
                **scanner_identities[label],
            }
            for label, result in versions.items()
        },
        "services": {
            "rust": {
                "repository": security["rustsec_repository"],
                "database": rust_report.get("database"),
            },
            "npm": {
                "registry": security["npm_registry"],
                "observation": (
                    "registry response has no stable advisory-database revision; "
                    "bind report digest and time"
                ),
            },
            "python": {
                "service": security["python_advisory_service"],
                "observation": (
                    "service response has no stable advisory-database revision; "
                    "bind report digest and time"
                ),
            },
            "go": go_configuration,
        },
        "findings": {
            "rust": rust_vulnerabilities,
            "npm": npm_vulnerabilities,
            "python": python_vulnerabilities,
            "go": go_findings,
        },
        "reports": reports,
        "passed": successful,
    }
    receipt_path = output / "receipt.json"
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(receipt_path)
    if not successful:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
