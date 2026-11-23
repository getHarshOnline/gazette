#!/usr/bin/env python3
"""Generate Gazette's deterministic source-distribution evidence offline."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import subprocess
import tempfile
import textwrap
import tomllib
import urllib.parse
import uuid
from pathlib import Path, PurePosixPath
from typing import Any


SPDX_VERSION = "SPDX-2.3"
DATA_LICENSE = "CC0-1.0"
DOCUMENT_SEED = uuid.UUID("a587f6ef-1d17-51bd-9d45-d2fa7e24e72b")
OUTPUTS = (
    "NOTICE",
    "THIRD_PARTY_NOTICES.md",
    "SBOM.spdx.json",
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def confined(raw: str) -> PurePosixPath:
    path = PurePosixPath(raw)
    if path.is_absolute() or not path.parts or any(part in {"", ".", ".."} for part in path.parts):
        raise ValueError(f"unconfined relative path: {raw!r}")
    return path


def regular_file(root: Path, raw: str, expected: str | None = None) -> Path:
    relative = confined(raw)
    path = root.joinpath(*relative.parts)
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"missing regular file: {relative.as_posix()}")
    if not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f"file escapes root: {relative.as_posix()}")
    if expected is not None:
        actual = sha256(path.read_bytes())
        if actual != expected:
            raise ValueError(
                f"SHA-256 drift for {relative.as_posix()}: expected={expected} actual={actual}"
            )
    return path


def run(*args: str, cwd: Path) -> str:
    completed = subprocess.run(
        args,
        cwd=cwd,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return completed.stdout


def token(*parts: str) -> str:
    return "SPDXRef-" + re.sub(r"[^A-Za-z0-9.-]", "-", "-".join(parts))


def normalized_spdx_license(raw: str) -> str:
    return {
        "Apache-2.0/MIT": "Apache-2.0 OR MIT",
        "MIT / Apache-2.0": "MIT OR Apache-2.0",
        "MIT/Apache-2.0": "MIT OR Apache-2.0",
        "Unlicense/MIT": "Unlicense OR MIT",
    }.get(raw, raw)


def checksum_from_integrity(integrity: str) -> dict[str, str] | None:
    if integrity.startswith("sha512-"):
        value = base64.b64decode(integrity.removeprefix("sha512-"), validate=True).hex()
        return {"algorithm": "SHA512", "checksumValue": value}
    if integrity.startswith("h1:"):
        value = base64.b64decode(integrity.removeprefix("h1:"), validate=True).hex()
        return {"algorithm": "SHA256", "checksumValue": value}
    return None


def npm_purl(name: str, version: str) -> str:
    if name.startswith("@") and "/" in name:
        namespace, package_name = name.split("/", 1)
        encoded_name = (
            f"{urllib.parse.quote(namespace, safe='')}/"
            f"{urllib.parse.quote(package_name, safe='')}"
        )
    else:
        encoded_name = urllib.parse.quote(name, safe="")
    return f"pkg:npm/{encoded_name}@{version}"


def package(
    spdx_id: str,
    name: str,
    version: str,
    raw_license: str,
    comment: str,
    *,
    checksum: dict[str, str] | None = None,
    purl: str | None = None,
    source: str = "NOASSERTION",
    purpose: str = "LIBRARY",
) -> dict[str, Any]:
    exact = raw_license or "NOASSERTION"
    declared = normalized_spdx_license(exact)
    comment += f"; exact upstream licence metadata={exact!r}"
    if declared != exact:
        comment += f"; SPDX syntax={declared!r}"
    row: dict[str, Any] = {
        "SPDXID": spdx_id,
        "name": name,
        "versionInfo": version,
        "downloadLocation": source,
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": declared,
        "copyrightText": "NOASSERTION",
        "primaryPackagePurpose": purpose,
        "comment": comment,
    }
    if checksum is not None:
        row["checksums"] = [checksum]
    if purl is not None:
        row["externalRefs"] = [
            {
                "referenceCategory": "PACKAGE-MANAGER",
                "referenceType": "purl",
                "referenceLocator": purl,
            }
        ]
    return row


def cargo_identity(item: dict[str, Any], source_root: Path) -> str:
    """Identify local packages by their source-relative home, never the checkout path."""
    source = item.get("source")
    if source is None:
        manifest = Path(item["manifest_path"]).resolve(strict=True)
        try:
            relative = manifest.relative_to(source_root.resolve(strict=True)).as_posix()
        except ValueError as error:
            raise ValueError("local Cargo package is outside the source distribution") from error
        source = "path:" + relative
    return json.dumps([source, item["name"], item["version"]], separators=(",", ":"))


def cargo_inventory(
    source_root: Path, config: dict[str, Any]
) -> tuple[list[dict[str, Any]], list[dict[str, str]], dict[str, dict[str, Any]], set[str]]:
    selected = config["cargo"]
    lock_path = regular_file(source_root, selected["lock"], selected["sha256"])
    lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    checksums = {
        (row["name"], row["version"], row.get("source")): row.get("checksum")
        for row in lock["package"]
    }
    metadata = json.loads(
        run(
            "cargo",
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            cwd=source_root,
        )
    )
    workspace = set(metadata["workspace_members"])
    packages: list[dict[str, Any]] = []
    by_cargo_id: dict[str, dict[str, Any]] = {}
    for item in metadata["packages"]:
        source = item.get("source")
        checksum = checksums.get((item["name"], item["version"], source))
        digest = sha256(cargo_identity(item, source_root).encode())[:12]
        spdx_id = token("cargo", item["name"], item["version"], digest)
        purl = None
        if source and source.startswith("registry+"):
            purl = f"pkg:cargo/{urllib.parse.quote(item['name'], safe='')}@{item['version']}"
        row = package(
            spdx_id,
            item["name"],
            item["version"],
            item.get("license") or "NOASSERTION",
            "Gazette workspace crate"
            if item["id"] in workspace
            else "Cargo.lock source dependency",
            checksum={"algorithm": "SHA256", "checksumValue": checksum}
            if checksum
            else None,
            purl=purl,
            source=source if source and source.startswith("git+") else "NOASSERTION",
        )
        row["_cargo_id"] = item["id"]
        row["_manifest_root"] = str(Path(item["manifest_path"]).parent)
        packages.append(row)
        by_cargo_id[item["id"]] = row

    relationships: list[dict[str, str]] = []
    for item in metadata["resolve"]["nodes"]:
        for dependency in item["deps"]:
            relationships.append(
                {
                    "spdxElementId": by_cargo_id[item["id"]]["SPDXID"],
                    "relationshipType": "DEPENDS_ON",
                    "relatedSpdxElement": by_cargo_id[dependency["pkg"]]["SPDXID"],
                }
            )
    return packages, relationships, by_cargo_id, workspace


def cargo_name_map(cargo: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    matches: dict[str, list[dict[str, Any]]] = {}
    for row in cargo:
        matches.setdefault(row["name"], []).append(row)
    return {name: rows[0] for name, rows in matches.items() if len(rows) == 1}


def node_inventory(
    source_root: Path, rows: list[dict[str, Any]]
) -> tuple[list[dict[str, Any]], list[dict[str, str]], list[dict[str, str]]]:
    packages: list[dict[str, Any]] = []
    relationships: list[dict[str, str]] = []
    evidence: list[dict[str, str]] = []
    for selected in rows:
        lock_path = regular_file(source_root, selected["path"], selected["sha256"])
        lock = json.loads(lock_path.read_text(encoding="utf-8"))
        scoped: list[dict[str, Any]] = []
        root_id = ""
        for path, item in sorted(lock.get("packages", {}).items()):
            name = lock["name"] if path == "" else path.rsplit("node_modules/", 1)[-1]
            version = str(item.get("version") or lock.get("version") or "NOASSERTION")
            raw_license = (
                item.get("license")
                or (selected.get("root_license") if path == "" else None)
                or "NOASSERTION"
            )
            spdx_id = token(
                "npm",
                name,
                version,
                sha256(f"{selected['path']}\0{path}".encode())[:12],
            )
            integrity = item.get("integrity")
            row = package(
                spdx_id,
                name,
                version,
                raw_license,
                f"npm lock source; lock={selected['path']}; role={selected['role']}; dev={bool(item.get('dev'))}",
                checksum=checksum_from_integrity(integrity) if integrity else None,
                purl=npm_purl(name, version),
                source=item.get("resolved", "NOASSERTION"),
            )
            scoped.append(row)
            if path == "":
                root_id = spdx_id
        if not root_id:
            raise ValueError(f"npm lock has no root package: {selected['path']}")
        for row in scoped:
            if row["SPDXID"] != root_id:
                relationships.append(
                    {
                        "spdxElementId": root_id,
                        "relationshipType": "DEPENDS_ON",
                        "relatedSpdxElement": row["SPDXID"],
                        "comment": "member of exact lockfile closure; not asserted as a direct edge",
                    }
                )
        packages.extend(scoped)
        evidence.append(
            {
                "kind": "npm",
                "path": selected["path"],
                "sha256": selected["sha256"],
                "role": selected["role"],
                "packages": str(len(scoped)),
            }
        )
    return packages, relationships, evidence


def python_inventory(
    source_root: Path, rows: list[dict[str, Any]]
) -> tuple[list[dict[str, Any]], list[dict[str, str]], list[dict[str, str]]]:
    packages: list[dict[str, Any]] = []
    relationships: list[dict[str, str]] = []
    evidence: list[dict[str, str]] = []
    for selected in rows:
        path = regular_file(source_root, selected["path"], selected["sha256"])
        logical = re.sub(r"\\\s*\n\s*", " ", path.read_text(encoding="utf-8"))
        root_id = token("toolchain", "sphinx", sha256(selected["path"].encode())[:12])
        packages.append(
            package(
                root_id,
                "Gazette Sphinx provider qualification",
                "0.1.0",
                "NOASSERTION",
                selected["role"],
                purpose="APPLICATION",
            )
        )
        count = 0
        for line in logical.splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            match = re.match(r"([A-Za-z0-9_.-]+)==([^\s]+)", line)
            if not match:
                raise ValueError(
                    f"unsupported Python lock row in {selected['path']}: {line!r}"
                )
            name, version = match.groups()
            hashes = sorted(
                set(re.findall(r"--hash=sha256:([0-9a-f]{64})", line))
            )
            if not hashes:
                raise ValueError(f"Python lock row lacks SHA-256: {name}=={version}")
            spdx_id = token(
                "pypi", name, version, sha256(selected["path"].encode())[:8]
            )
            packages.append(
                package(
                    spdx_id,
                    name,
                    version,
                    "NOASSERTION",
                    f"hash-locked Python provider dependency; lock={selected['path']}; all hashes={','.join(hashes)}",
                    checksum={"algorithm": "SHA256", "checksumValue": hashes[0]},
                    purl=f"pkg:pypi/{urllib.parse.quote(name.lower().replace('_', '-'), safe='')}@{version}",
                )
            )
            relationships.append(
                {
                    "spdxElementId": root_id,
                    "relationshipType": "DEPENDS_ON",
                    "relatedSpdxElement": spdx_id,
                }
            )
            count += 1
        evidence.append(
            {
                "kind": "python",
                "path": selected["path"],
                "sha256": selected["sha256"],
                "role": selected["role"],
                "packages": str(count + 1),
            }
        )
    return packages, relationships, evidence


def go_inventory(
    source_root: Path, rows: list[dict[str, Any]]
) -> tuple[list[dict[str, Any]], list[dict[str, str]], list[dict[str, str]]]:
    packages: list[dict[str, Any]] = []
    relationships: list[dict[str, str]] = []
    evidence: list[dict[str, str]] = []
    for selected in rows:
        module_path = regular_file(
            source_root, selected["module"], selected["module_sha256"]
        )
        sum_path = regular_file(source_root, selected["sum"], selected["sum_sha256"])
        sums: dict[tuple[str, str], str] = {}
        for line in sum_path.read_text(encoding="utf-8").splitlines():
            parts = line.split()
            if len(parts) == 3 and not parts[1].endswith("/go.mod"):
                sums[(parts[0], parts[1])] = parts[2]
        text = module_path.read_text(encoding="utf-8")
        module_match = re.search(r"^module\s+(\S+)$", text, re.MULTILINE)
        if not module_match:
            raise ValueError(f"Go module path is absent: {selected['module']}")
        root_id = token("go-toolchain", sha256(selected["module"].encode())[:12])
        packages.append(
            package(
                root_id,
                module_match.group(1),
                "0.1.0-local",
                "NOASSERTION",
                selected["role"],
                purpose="APPLICATION",
            )
        )
        block = re.search(r"require\s*\((.*?)\n\)", text, re.DOTALL)
        if not block:
            raise ValueError(f"Go dependency block is absent: {selected['module']}")
        count = 0
        for line in block.group(1).splitlines():
            clean = line.split("//", 1)[0].strip()
            if not clean:
                continue
            name, version = clean.split()[:2]
            integrity = sums.get((name, version))
            if integrity is None:
                raise ValueError(f"Go module has no content sum: {name}@{version}")
            spdx_id = token(
                "golang", name, version, sha256(selected["module"].encode())[:8]
            )
            packages.append(
                package(
                    spdx_id,
                    name,
                    version,
                    "NOASSERTION",
                    f"readonly Go provider dependency; module={selected['module']}; exact go.sum={integrity}",
                    checksum=checksum_from_integrity(integrity),
                    purl=f"pkg:golang/{urllib.parse.quote(name, safe='/')}@{urllib.parse.quote(version, safe='')}",
                )
            )
            relationships.append(
                {
                    "spdxElementId": root_id,
                    "relationshipType": "DEPENDS_ON",
                    "relatedSpdxElement": spdx_id,
                }
            )
            count += 1
        evidence.append(
            {
                "kind": "go",
                "path": f"{selected['module']} + {selected['sum']}",
                "sha256": f"{selected['module_sha256']} + {selected['sum_sha256']}",
                "role": selected["role"],
                "packages": str(count + 1),
            }
        )
    return packages, relationships, evidence


def asset_inventory(
    source_root: Path,
    rows: list[dict[str, Any]],
    cargo_by_name: dict[str, dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, str]], list[dict[str, Any]]]:
    packages: list[dict[str, Any]] = []
    relationships: list[dict[str, str]] = []
    evidence: list[dict[str, Any]] = []
    for selected in rows:
        regular_file(
            source_root, selected["license_path"], selected["license_sha256"]
        )
        files = []
        for item in selected["files"]:
            regular_file(source_root, item["path"], item["sha256"])
            files.append(dict(item))
        spdx_id = token("asset", selected["id"], selected["version"])
        comment = (
            f"{selected['role']}; retained licence={selected['license_path']} sha256={selected['license_sha256']}; files="
            + ",".join(
                f"{item['path']}@sha256:{item['sha256']}" for item in files
            )
        )
        packages.append(
            package(
                spdx_id,
                selected["name"],
                selected["version"],
                selected["license"],
                comment,
                source=selected["source"],
                purpose="SOURCE",
            )
        )
        owner_name = selected.get("owner")
        if owner_name is None:
            relationships.append(
                {
                    "spdxElementId": "SPDXRef-DOCUMENT",
                    "relationshipType": "DESCRIBES",
                    "relatedSpdxElement": spdx_id,
                }
            )
        else:
            owner = cargo_by_name.get(owner_name)
            if owner is None:
                raise ValueError(
                    f"asset owner is not one exact Cargo package: {owner_name}"
                )
            relationships.append(
                {
                    "spdxElementId": owner["SPDXID"],
                    "relationshipType": "CONTAINS",
                    "relatedSpdxElement": spdx_id,
                }
            )
        evidence.append({**selected, "files": files})
    return packages, relationships, evidence


def registry_evidence(
    row: dict[str, Any], relative: str, expected: str
) -> tuple[Path, str]:
    root = Path(row["_manifest_root"])
    path = regular_file(root, relative, expected)
    return path, path.read_text(encoding="utf-8")


def retained_notices(
    source_root: Path,
    rows: list[dict[str, Any]], cargo_by_name: dict[str, dict[str, Any]]
) -> list[dict[str, Any]]:
    retained = []
    for selected in rows:
        carrier = cargo_by_name.get(selected["package"])
        if carrier is None or carrier["versionInfo"] != selected["version"]:
            raise ValueError(
                f"retained notice package mismatch: {selected['package']}@{selected['version']}"
            )
        if "decoded_path" in selected:
            regular_file(
                Path(carrier["_manifest_root"]), selected["path"], selected["sha256"]
            )
            text = regular_file(
                source_root, selected["decoded_path"], selected["decoded_sha256"]
            ).read_text(encoding="utf-8")
        else:
            _, text = registry_evidence(
                carrier, selected["path"], selected["sha256"]
            )
        retained.append({**selected, "text": text})
    return retained


def native_inventory(
    rows: list[dict[str, Any]], cargo_by_name: dict[str, dict[str, Any]]
) -> tuple[
    list[dict[str, Any]],
    list[dict[str, str]],
    list[dict[str, Any]],
    list[dict[str, Any]],
]:
    packages: list[dict[str, Any]] = []
    relationships: list[dict[str, str]] = []
    evidence: list[dict[str, Any]] = []
    extracted: list[dict[str, Any]] = []
    for selected in rows:
        carrier = cargo_by_name.get(selected["carrier"])
        if carrier is None or carrier["versionInfo"] != selected["carrier_version"]:
            raise ValueError(
                f"native carrier mismatch: {selected['carrier']}@{selected['carrier_version']}"
            )
        _, native_text = registry_evidence(
            carrier, selected["evidence_path"], selected["evidence_sha256"]
        )
        marker = selected.get("evidence_contains")
        if marker and marker not in native_text:
            raise ValueError(f"native evidence marker is absent for {selected['id']}")
        license_text = (
            native_text
            if selected.get("license_from_evidence")
            else selected["license_text"]
        )
        spdx_id = token("native", selected["id"], selected["version"])
        packages.append(
            package(
                spdx_id,
                selected["name"],
                selected["version"],
                selected["license"],
                f"{selected['role']}; carrier={selected['carrier']}@{selected['carrier_version']}; evidence={selected['evidence_path']} sha256={selected['evidence_sha256']}",
                source=selected["source"],
            )
        )
        relationships.append(
            {
                "spdxElementId": carrier["SPDXID"],
                "relationshipType": "CONTAINS",
                "relatedSpdxElement": spdx_id,
            }
        )
        extracted.append(
            {
                "licenseId": selected["license"],
                "extractedText": license_text,
                "name": f"{selected['name']} retained licence evidence",
                "seeAlsos": [selected["source"]],
            }
        )
        evidence.append({**selected, "text": license_text})
    return packages, relationships, evidence, extracted


def exception_text(product: dict[str, Any]) -> str:
    lines = []
    for exception in product.get("exceptions", []):
        paths = [f"{path}/" for path in exception["paths"]]
        named = ", ".join(paths[:-1]) + f" and {paths[-1]}" if len(paths) > 1 else paths[0]
        lines.append(
            f"The {exception['role']} under {named} are licensed under "
            f"{exception['license']} instead; each carries its own LICENSE and NOTICE."
        )
    return "".join(f"\n{textwrap.fill(line, 80)}\n" for line in lines)


def ownership_text(product: dict[str, Any]) -> str:
    lines = []
    if "copyright" in product:
        lines.append(f"Copyright © {product['copyright']}.")
    if "author" in product:
        lines.append(f"Author and originator: {product['author']}.")
    return "".join(f"{line}\n" for line in lines)


def notice_text(product: dict[str, Any]) -> str:
    return f"""{product['name']}
{ownership_text(product)}
This repository declares {product['name']} {product['version']} under
{product['license']} in its root package manifest. See LICENSE for the complete
licence text.
{exception_text(product)}
Third-party source dependencies, retained assets and native components preserve
their own licences, notices and ownership. Their generated human-readable
inventory is THIRD_PARTY_NOTICES.md; its machine-readable counterpart is
SBOM.spdx.json. Each release archive carries both beside this file, generated
from the locked inputs at the release; the release also attaches a separate
binary SBOM.

These files are deterministic mechanical source evidence. They do not establish
legal approval, binary contents, trademark clearance, publication permission,
release authorization or alternative licence terms.
"""


def exact_license(row: dict[str, Any]) -> str:
    marker = "exact upstream licence metadata='"
    start = row["comment"].rfind(marker)
    if start == -1:
        return row["licenseDeclared"]
    start += len(marker)
    return row["comment"][start:].split("'", 1)[0]


def notice_literal(text: str) -> list[str]:
    length = max((len(run) for run in re.findall(r"`+", text)), default=0)
    fence = "`" * max(3, length + 1)
    return [fence + "text", text.rstrip(), fence]


def notices_text(
    config: dict[str, Any],
    cargo: list[dict[str, Any]],
    ecosystem: list[dict[str, str]],
    assets: list[dict[str, Any]],
    retained: list[dict[str, Any]],
    native: list[dict[str, Any]],
) -> str:
    product = config["product"]
    lines = [
        f"# {product['name']} third-party notices and source inventory",
        "",
        "Generated offline from exact selected inputs by",
        "`tools/generate_distribution_evidence.py`.",
        f"Inventory metadata timestamp: `{config['created']}`. This fixed timestamp",
        "makes generation deterministic; it is not an execution or qualification date.",
        "",
        f"{product['name']} declares `{product['license']}` in its root manifest. Every",
        "upstream licence spelling below is preserved exactly as supplied by that",
        "ecosystem. SPDX machine fields normalize only known legacy slash syntax; the",
        "original spelling remains in each package comment.",
        "",
        "This is mechanical source and toolchain evidence, not legal approval, a",
        "source-to-binary closure, publication permission or release authorization.",
        "Provider-qualification toolchains are inventoried separately and are not",
        "asserted to be shipped runtime components. `NOASSERTION` remains an explicit",
        "review requirement wherever that component is selected for distribution.",
        "",
        "## Locked input closures",
        "",
        "| ecosystem | exact input | SHA-256 | role | packages |",
        "|---|---|---|---|---:|",
        f"| Cargo | `{config['cargo']['lock']}` | `{config['cargo']['sha256']}` | Gazette Rust source graph | {len(cargo)} |",
    ]
    for row in ecosystem:
        lines.append(
            f"| {row['kind']} | `{row['path']}` | `{row['sha256']}` | {row['role']} | {row['packages']} |"
        )
    lines.extend(
        [
            "",
            "## Cargo workspace and source dependencies",
            "",
            "| package | version | exact declared licence | kind | source identity |",
            "|---|---|---|---|---|",
        ]
    )
    workspace_ids = {
        item["_cargo_id"] for item in cargo if "workspace crate" in item["comment"]
    }
    for row in sorted(
        cargo, key=lambda item: (item["name"], item["versionInfo"], item["SPDXID"])
    ):
        kind = "workspace" if row["_cargo_id"] in workspace_ids else "dependency"
        checksums = row.get("checksums", [])
        checksum = (
            f"`sha256:{checksums[0]['checksumValue']}`"
            if checksums
            else f"`{row['downloadLocation']}`"
            if row["downloadLocation"] != "NOASSERTION"
            else "—"
        )
        lines.append(
            f"| `{row['name']}` | `{row['versionInfo']}` | `{exact_license(row)}` | {kind} | {checksum} |"
        )
    lines.extend(
        [
            "",
            "## Retained source assets",
            "",
            "| asset | version | exact licence | retained evidence | role |",
            "|---|---|---|---|---|",
        ]
    )
    for row in assets:
        files = "; ".join(
            f"`{item['path']}` sha256 `{item['sha256']}`" for item in row["files"]
        )
        lines.append(
            f"| `{row['name']}` | `{row['version']}` | `{row['license']}` — `{row['license_path']}` sha256 `{row['license_sha256']}` | {files} | {row['role']} |"
        )
    lines.extend(
        [
            "",
            "## Bundled native components",
            "",
            "| component | version | exact licence | carrier and evidence | role |",
            "|---|---|---|---|---|",
        ]
    )
    for row in native:
        lines.append(
            f"| `{row['name']}` | `{row['version']}` | `{row['license']}` | `{row['carrier']}@{row['carrier_version']}`; `{row['evidence_path']}` sha256 `{row['evidence_sha256']}` | {row['role']} |"
        )
    lines.extend(
        [
            "",
            "## Complete retained redistribution notices",
            "",
            "The following text is copied mechanically from exact checksum-verified",
            "Cargo source, its retained decoded asset notices or its verified embedded native notice. It is reproduced",
            "without editorial interpretation.",
        ]
    )
    for row in retained:
        lines.extend(
            [
                "",
                f"### `{row['package']}@{row['version']}` — {row['reason']}",
                "",
                *notice_literal(row["text"]),
            ]
        )
    for row in native:
        lines.extend(
            [
                "",
                f"### `{row['name']}@{row['version']}`",
                "",
                *notice_literal(row["text"]),
            ]
        )
    lines.extend(
        [
            "",
            "## Boundary",
            "",
            "Regenerate after any selected lock, retained asset or native carrier changes.",
            "A matching inventory does not approve distribution or prove what a compiled binary contains.",
            "",
        ]
    )
    return "\n".join(lines)


def build(source_root: Path, config: dict[str, Any]) -> dict[str, str]:
    cargo, relationships, cargo_by_id, workspace = cargo_inventory(source_root, config)
    cargo_by_name = cargo_name_map(cargo)
    node, node_edges, node_evidence = node_inventory(
        source_root, config.get("node_lock", [])
    )
    python, python_edges, python_evidence = python_inventory(
        source_root, config.get("python_lock", [])
    )
    go, go_edges, go_evidence = go_inventory(
        source_root, config.get("go_lock", [])
    )
    assets, asset_edges, asset_evidence = asset_inventory(
        source_root, config.get("asset", []), cargo_by_name
    )
    retained = retained_notices(
        source_root, config.get("retained_cargo_notice", []), cargo_by_name
    )
    native, native_edges, native_evidence, extracted = native_inventory(
        config.get("native_component", []), cargo_by_name
    )

    packages = cargo + node + python + go + assets + native
    public_packages = [
        {key: value for key, value in row.items() if not key.startswith("_")}
        for row in packages
    ]
    for cargo_id in sorted(workspace):
        relationships.append(
            {
                "spdxElementId": "SPDXRef-DOCUMENT",
                "relationshipType": "DESCRIBES",
                "relatedSpdxElement": cargo_by_id[cargo_id]["SPDXID"],
            }
        )
    relationships.extend(
        node_edges + python_edges + go_edges + asset_edges + native_edges
    )
    relationships.sort(
        key=lambda row: (
            row["spdxElementId"],
            row["relationshipType"],
            row["relatedSpdxElement"],
            row.get("comment", ""),
        )
    )
    public_packages.sort(key=lambda row: row["SPDXID"])

    input_hashes = [config["cargo"]["sha256"]]
    input_hashes.extend(row["sha256"] for row in node_evidence + python_evidence)
    input_hashes.extend(row["sha256"] for row in go_evidence)
    seed = json.dumps(
        {
            "packages": public_packages,
            "relationships": relationships,
            "input_sha256": input_hashes,
        },
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    )
    document_uuid = uuid.uuid5(DOCUMENT_SEED, seed)
    sbom = {
        "spdxVersion": SPDX_VERSION,
        "dataLicense": DATA_LICENSE,
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": "Gazette source distribution inventory",
        "documentNamespace": f"https://gazette.invalid/spdx/{document_uuid}",
        "creationInfo": {
            "created": config["created"],
            "creators": ["Tool: tools/generate_distribution_evidence.py"],
        },
        "documentComment": (
            f"Deterministic offline source and selected provider-toolchain inventory for Gazette {config['product']['version']}; "
            f"Cargo.lock sha256={config['cargo']['sha256']}. This is not legal approval, a binary SBOM, publication permission or release authorization. "
            "Provider-qualification tools are not asserted to be distributed runtime components."
        ),
        "hasExtractedLicensingInfos": sorted(
            extracted, key=lambda row: row["licenseId"]
        ),
        "packages": public_packages,
        "relationships": relationships,
    }
    if len({row["SPDXID"] for row in public_packages}) != len(public_packages):
        raise ValueError("duplicate SPDX package identifier")
    package_ids = {row["SPDXID"] for row in public_packages} | {
        "SPDXRef-DOCUMENT"
    }
    for edge in relationships:
        if (
            edge["spdxElementId"] not in package_ids
            or edge["relatedSpdxElement"] not in package_ids
        ):
            raise ValueError(f"SPDX relationship references unknown package: {edge}")
    json.loads(json.dumps(sbom, allow_nan=False))

    ecosystem = node_evidence + python_evidence + go_evidence
    return {
        "NOTICE": notice_text(config["product"]),
        "THIRD_PARTY_NOTICES.md": notices_text(
            config, cargo, ecosystem, asset_evidence, retained, native_evidence
        ),
        "SBOM.spdx.json": json.dumps(
            sbom, indent=2, sort_keys=True, ensure_ascii=False
        )
        + "\n",
    }


def atomic_write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: str | None = None
    try:
        with tempfile.NamedTemporaryFile(
            "w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            delete=False,
        ) as handle:
            temporary = handle.name
            handle.write(text)
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(temporary, 0o644)
        os.replace(temporary, path)
        temporary = None
    finally:
        if temporary is not None:
            Path(temporary).unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--source-root", type=Path, default=Path(__file__).resolve().parent.parent
    )
    parser.add_argument(
        "--config", type=Path, default=Path("tools/distribution-evidence.toml")
    )
    parser.add_argument("--output-root", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    source_root = args.source_root.resolve(strict=True)
    config_path = (
        args.config if args.config.is_absolute() else source_root / args.config
    )
    if not config_path.is_file() or config_path.is_symlink():
        raise SystemExit(f"missing regular distribution evidence config: {config_path}")
    config = tomllib.loads(config_path.read_text(encoding="utf-8"))
    if config.get("schema_version") != 1:
        raise SystemExit("unsupported distribution evidence schema")
    if args.output_root is None and not args.check:
        parser.error("generation requires --output-root in a release or temporary directory")
    output_root = args.output_root.resolve() if args.output_root else None
    if output_root == source_root:
        parser.error("generated distribution files must not replace source-root files")
    outputs = build(source_root, config)
    if output_root is None:
        print("Distribution inputs and generated inventory are valid.")
        return
    if args.check:
        stale = []
        for name in OUTPUTS:
            path = output_root / name
            if (
                not path.is_file()
                or path.is_symlink()
                or path.read_text(encoding="utf-8") != outputs[name]
            ):
                stale.append(name)
        if stale:
            raise SystemExit("stale distribution evidence: " + ", ".join(stale))
        return
    for name in OUTPUTS:
        atomic_write(output_root / name, outputs[name])


if __name__ == "__main__":
    main()
