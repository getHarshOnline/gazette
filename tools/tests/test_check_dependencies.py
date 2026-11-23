from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


RUNNER = Path(__file__).resolve().parents[1] / "check_dependencies.py"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class DependencySecurityQualificationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="gazette-security-test-")
        self.root = Path(self.temporary.name) / "source"
        self.root.mkdir()
        selected = {
            "Cargo.lock": "cargo",
            "vscode/package-lock.json": "node-one",
            "typedoc/package-lock.json": "node-two",
            "sphinx/requirements.txt": "python",
            "go/go.mod": "go-module",
            "go/go.sum": "go-sum",
            "tools/security/pip-audit-requirements.txt": "pip-audit==2.10.1 --hash=sha256:fixture",
        }
        for raw, body in selected.items():
            path = self.root / raw
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body, encoding="utf-8")
        config = textwrap.dedent(
            f"""
            [cargo]
            lock = "Cargo.lock"
            sha256 = "{sha256(self.root / 'Cargo.lock')}"

            [security_qualification]
            cargo_audit_version = "0.22.2"
            npm_version = "11.16.0"
            pip_audit_version = "2.10.1"
            govulncheck_module = "golang.org/x/vuln/cmd/govulncheck"
            govulncheck_version = "v1.8.0"
            go_toolchain_version = "go1.26.6"
            rustsec_repository = "https://github.com/RustSec/advisory-db"
            npm_registry = "https://registry.npmjs.org/"
            python_advisory_service = "pypi"
            go_vulnerability_database = "https://vuln.go.dev"

            [[scanner_lock]]
            path = "tools/security/pip-audit-requirements.txt"
            sha256 = "{sha256(self.root / 'tools/security/pip-audit-requirements.txt')}"

            [[node_lock]]
            path = "vscode/package-lock.json"
            sha256 = "{sha256(self.root / 'vscode/package-lock.json')}"

            [[node_lock]]
            path = "typedoc/package-lock.json"
            sha256 = "{sha256(self.root / 'typedoc/package-lock.json')}"

            [[python_lock]]
            path = "sphinx/requirements.txt"
            sha256 = "{sha256(self.root / 'sphinx/requirements.txt')}"

            [[go_lock]]
            module = "go/go.mod"
            module_sha256 = "{sha256(self.root / 'go/go.mod')}"
            sum = "go/go.sum"
            sum_sha256 = "{sha256(self.root / 'go/go.sum')}"
            """
        )
        (self.root / "config.toml").write_text(config, encoding="utf-8")
        self.bin = Path(self.temporary.name) / "bin"
        self.bin.mkdir()
        self.executables = {}
        for name in ("cargo-audit", "npm", "pip-audit", "govulncheck"):
            path = self.bin / name
            path.write_text(self.fake_scanner(name), encoding="utf-8")
            path.chmod(0o755)
            self.executables[name] = path

    def tearDown(self) -> None:
        self.temporary.cleanup()

    @staticmethod
    def fake_scanner(name: str) -> str:
        return "#!/usr/bin/env python3\n" + textwrap.dedent(
            f"""
            import json
            import os
            import sys
            from pathlib import Path

            name = {name!r}
            args = sys.argv[1:]
            if '--version' in args or '-version' in args:
                versions = {{
                    'cargo-audit': 'cargo-audit 0.22.2',
                    'npm': '11.16.0',
                    'pip-audit': 'pip-audit 2.10.1',
                    'govulncheck': 'Go: go1.26.6\\nScanner: govulncheck@v1.8.0',
                }}
                if name == 'pip-audit' and os.environ.get('FAKE_DECEPTIVE_VERSION') == '1':
                    versions[name] = 'pip-audit 2.10.10'
                print(versions[name])
                raise SystemExit(0)
            if name == 'cargo-audit':
                required = [
                    'audit', '--db', '--url', '--file', '--no-yanked', '--deny',
                    'warnings', '--json',
                ]
                assert all(value in args for value in required), args
                print(json.dumps({{
                    'database': {{'last-commit': 'abc', 'advisory-count': 1}},
                    'vulnerabilities': {{'count': 0}},
                }}))
            elif name == 'npm':
                assert args[:3] == ['audit', '--package-lock-only', '--json'], args
                assert '--registry' in args and 'https://registry.npmjs.org/' in args, args
                print(json.dumps({{'metadata': {{'vulnerabilities': {{'total': 0}}}}}}))
                if os.environ.get('FAKE_MUTATE_INPUT') == '1':
                    Path('package-lock.json').write_text('changed', encoding='utf-8')
            elif name == 'pip-audit':
                assert '--require-hashes' in args, args
                assert '--disable-pip' in args, args
                assert '--fix' not in args, args
                print(json.dumps({{'dependencies': [{{'name': 'sphinx', 'vulns': []}}]}}))
                if os.environ.get('FAKE_MUTATE_EXECUTABLE') == '1':
                    executable = Path(sys.argv[0])
                    executable.write_text(executable.read_text(encoding='utf-8') + '\\n# changed', encoding='utf-8')
            else:
                assert '-scan=module' in args and './...' not in args, args
                print(json.dumps({{'config': {{
                    'scanner_version': 'v1.8.0',
                    'db_last_modified': '2026-09-16T18:00:43Z',
                    'go_version': 'go1.26.6',
                }}}}, indent=2))
                print(json.dumps({{'SBOM': {{
                    'modules': [{{'path': 'example.test/tools'}}],
                }}}}, indent=2))
                if os.environ.get('FAKE_GO_FINDING') == '1':
                    print(json.dumps({{'finding': {{'osv': 'GO-TEST-0001'}}}}, indent=2))
                    raise SystemExit(3)
            """
        )

    def invoke(
        self, *, finding: bool = False, mutate: bool = False,
        deceptive_version: bool = False, mutate_executable: bool = False,
    ) -> subprocess.CompletedProcess[str]:
        label = "mutated" if mutate else "failed" if finding else "passed"
        output = Path(self.temporary.name) / label
        environment = dict(os.environ)
        if finding:
            environment["FAKE_GO_FINDING"] = "1"
        if mutate:
            environment["FAKE_MUTATE_INPUT"] = "1"
        if deceptive_version:
            environment["FAKE_DECEPTIVE_VERSION"] = "1"
        if mutate_executable:
            environment["FAKE_MUTATE_EXECUTABLE"] = "1"
        return subprocess.run(
            [
                sys.executable,
                str(RUNNER),
                "--source-root", str(self.root),
                "--config", "config.toml",
                "--output-dir", str(output),
                "--cargo-audit", str(self.executables["cargo-audit"]),
                "--npm", str(self.executables["npm"]),
                "--pip-audit", str(self.executables["pip-audit"]),
                "--govulncheck", str(self.executables["govulncheck"]),
            ],
            env=environment,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

    def test_zero_findings_produces_bound_external_receipt(self) -> None:
        result = self.invoke(finding=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        receipt = json.loads((Path(result.stdout.strip())).read_text(encoding="utf-8"))
        self.assertTrue(receipt["passed"])
        self.assertEqual(receipt["findings"], {"go": 0, "npm": 0, "python": 0, "rust": 0})
        self.assertIn("excluded from deterministic build", receipt["boundary"])
        self.assertEqual(len(receipt["inputs"]), 7)
        self.assertEqual(
            set(receipt["scanners"]),
            {"cargo_audit", "npm", "pip_audit", "govulncheck"},
        )
        for scanner in receipt["scanners"].values():
            self.assertEqual(len(scanner["sha256"]), 64)
            self.assertGreater(scanner["bytes"], 0)

    def test_finding_fails_closed_but_retains_receipt(self) -> None:
        result = self.invoke(finding=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        receipt_path = Path(result.stdout.strip())
        self.assertTrue(receipt_path.is_file())
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        self.assertFalse(receipt["passed"])
        self.assertEqual(receipt["findings"]["go"], 1)

    def test_input_mutation_fails_closed(self) -> None:
        result = self.invoke(mutate=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("dependency input drift", result.stderr)

    def test_deceptive_version_substring_is_rejected(self) -> None:
        result = self.invoke(deceptive_version=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("version mismatch", result.stderr)

    def test_changed_scanner_executable_is_rejected(self) -> None:
        result = self.invoke(mutate_executable=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("scanner executable changed", result.stderr)

    def test_scanner_closure_drift_is_rejected(self) -> None:
        scanner_lock = self.root / "tools/security/pip-audit-requirements.txt"
        original = scanner_lock.read_text(encoding="utf-8")
        scanner_lock.write_text(original + "\n# drift", encoding="utf-8")
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("dependency input drift", result.stderr)


if __name__ == "__main__":
    unittest.main()
