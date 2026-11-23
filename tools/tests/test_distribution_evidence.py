"""Release inventories retain identity and relationships when a checkout moves."""

import importlib.util
import json
import contextlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "distribution", Path(__file__).resolve().parents[1] / "generate_distribution_evidence.py"
)
distribution = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(distribution)


class CargoInventory(unittest.TestCase):
    def fixture(self, root):
        (root / "Cargo.lock").write_text("version = 4\npackage = []\n")
        packages = []
        for directory in ["crates/one", "vendor/one"]:
            manifest = root / directory / "Cargo.toml"
            manifest.parent.mkdir(parents=True)
            manifest.write_text('[package]\nname="one"\nversion="1.0.0"\n')
            packages.append({"name": "one", "version": "1.0.0", "source": None,
                             "id": manifest.parent.as_uri() + "#one@1.0.0",
                             "manifest_path": str(manifest), "license": "MIT"})
        for source, version in [("registry+https://registry.example/index", "1.0.0"),
                                ("git+https://git.example/one?rev=abc#abc", "1.0.0"),
                                ("git+https://git.example/one?rev=def#def", "1.0.0"),
                                ("registry+https://registry.example/index", "2.0.0")]:
            packages.append({"name": "one", "version": version, "source": source,
                             "id": source + "#one@" + version,
                             "manifest_path": str(root / "cache/Cargo.toml"),
                             "license": "MIT/Apache-2.0"})
        metadata = {"packages": packages, "workspace_members": [packages[0]["id"]],
                    "resolve": {"nodes": [{"id": p["id"], "deps": [{"pkg": q["id"]}
                                 for q in packages[i + 1:]]} for i, p in enumerate(packages)]}}
        config = {"cargo": {"lock": "Cargo.lock", "sha256": distribution.sha256(
            (root / "Cargo.lock").read_bytes())}}
        return metadata, config

    def inventory(self, root, metadata, config):
        with patch.object(distribution, "run", return_value=json.dumps(metadata)):
            packages, edges, _, _ = distribution.cargo_inventory(root, config)
        public = [{k: v for k, v in p.items() if not k.startswith("_")} for p in packages]
        return public, edges

    def test_relocation_preserves_every_package_and_dependency(self):
        with tempfile.TemporaryDirectory() as temp:
            first, second = Path(temp) / "first", Path(temp) / "other place"
            first.mkdir(); second.mkdir()
            a, ac = self.fixture(first)
            b, bc = self.fixture(second)
            left = self.inventory(first, a, ac)
            right = self.inventory(second, b, bc)
            self.assertEqual(left, right)
            self.assertEqual(len({p["SPDXID"] for p in left[0]}), 6)
            self.assertEqual(len(left[1]), 15)
            self.assertEqual(left[0][2]["licenseDeclared"], "MIT OR Apache-2.0")
            self.assertIn("MIT/Apache-2.0", left[0][2]["comment"])
            self.assertEqual(left[0][3]["downloadLocation"], a["packages"][3]["source"])
            self.assertNotIn(str(first), json.dumps(left))

    def test_local_package_outside_distribution_is_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "source"; root.mkdir()
            outside = Path(temp) / "Cargo.toml"; outside.write_text("")
            metadata, config = self.fixture(root)
            metadata["packages"][0]["manifest_path"] = str(outside)
            with self.assertRaisesRegex(ValueError, "outside the source distribution"):
                self.inventory(root, metadata, config)

    def test_input_drift_is_not_hidden_by_temporary_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            metadata, config = self.fixture(root)
            (root / "Cargo.lock").write_text("version = 4\npackage = []\n# changed\n")
            with self.assertRaisesRegex(ValueError, "SHA-256 drift"):
                self.inventory(root, metadata, config)


class RetainedNotices(unittest.TestCase):
    def test_embedded_markdown_cannot_end_the_notice_block(self):
        text = "Original terms\n````text\nA nested example\n````\n"
        config = {"product": {"name": "Example", "license": "MIT"}, "created": "2026-11-17",
                  "cargo": {"lock": "Cargo.lock", "sha256": "0" * 64}}
        notice = {"package": "syntaxes", "version": "1", "reason": "embedded notices", "text": text}
        output = distribution.notices_text(config, [], [], [], [notice], [])
        self.assertIn("`````text\n" + text + "`````\n", output)

    def test_decoded_notice_binds_the_package_binary_and_retained_text(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "source"; root.mkdir()
            registry = Path(temp) / "registry"; registry.mkdir()
            binary = registry / "notices.bin"; binary.write_bytes(b"\xff\x00packed notices")
            decoded = root / "NOTICES.md"; decoded.write_text("Original terms: café\n")
            package = {"versionInfo": "1.2.3", "_manifest_root": str(registry)}
            selected = {"package": "syntaxes", "version": "1.2.3", "path": "notices.bin",
                        "sha256": distribution.sha256(binary.read_bytes()),
                        "decoded_path": "NOTICES.md", "decoded_sha256": distribution.sha256(decoded.read_bytes()),
                        "reason": "embedded notices"}
            def read(row=selected, carrier=package):
                return distribution.retained_notices(root, [row], {"syntaxes": carrier})
            self.assertEqual(read()[0]["text"], "Original terms: café\n")
            with self.assertRaisesRegex(ValueError, "package mismatch"):
                read(carrier={**package, "versionInfo": "1.2.4"})
            with self.assertRaisesRegex(ValueError, "SHA-256 drift"):
                read({**selected, "sha256": "0" * 64})
            with self.assertRaisesRegex(ValueError, "SHA-256 drift"):
                read({**selected, "decoded_sha256": "0" * 64})
            with self.assertRaisesRegex(ValueError, "unconfined"):
                read({**selected, "decoded_path": "../registry/notices.bin"})
            decoded.unlink()
            with self.assertRaisesRegex(ValueError, "missing regular file"):
                read()


class OutputPolicy(unittest.TestCase):
    def test_check_does_not_rewrite_held_files_and_detects_changed_package_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "source"; root.mkdir()
            (root / "tools").mkdir()
            (root / "tools/distribution-evidence.toml").write_text("schema_version=1\n")
            held = root / "SBOM.spdx.json"; held.write_text("held original")
            output = Path(temp) / "release"
            generated = {name: "generated " + name for name in distribution.OUTPUTS}
            def invoke(*args):
                with patch("sys.argv", ["generate", "--source-root", str(root), *args]), \
                     patch.object(distribution, "build", return_value=generated), \
                     contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    distribution.main()
            invoke("--check")
            self.assertEqual(held.read_text(), "held original")
            self.assertFalse(output.exists())
            with self.assertRaises(SystemExit):
                invoke()
            with self.assertRaises(SystemExit):
                invoke("--output-root", str(root))
            invoke("--output-root", str(output))
            invoke("--check", "--output-root", str(output))
            (output / "NOTICE").write_text("changed")
            with self.assertRaisesRegex(SystemExit, "stale distribution evidence: NOTICE"):
                invoke("--check", "--output-root", str(output))
            self.assertEqual(held.read_text(), "held original")



class ProductNotice(unittest.TestCase):
    def test_licence_exceptions_name_each_path_and_its_licence(self):
        product = {"name": "Gazette", "version": "0.1.0", "license": "AGPL-3.0-or-later",
                   "exceptions": [{"role": "browser modules", "paths": ["sdk/a", "sdk/b"], "license": "Apache-2.0"}]}
        notice = distribution.notice_text(product)
        self.assertIn("under AGPL-3.0-or-later in its root package manifest", notice.replace("\n", " "))
        self.assertIn("The browser modules under sdk/a/ and sdk/b/ are licensed under Apache-2.0 instead", notice.replace("\n", " "))

    def test_a_product_without_exceptions_keeps_its_single_licence(self):
        notice = distribution.notice_text({"name": "Gazette", "version": "0.1.0", "license": "AGPL-3.0-or-later"})
        self.assertNotIn("instead", notice)

    def test_ownership_lines_follow_the_name_and_stay_absent_when_unconfigured(self):
        product = {"name": "Gazette", "version": "0.1.0", "license": "AGPL-3.0-or-later",
                   "copyright": "2026 Example Owner", "author": "A. Writer"}
        notice = distribution.notice_text(product)
        self.assertTrue(notice.startswith(
            "Gazette\nCopyright © 2026 Example Owner.\nAuthor and originator: A. Writer.\n\nThis repository"))
        bare = distribution.notice_text({"name": "Gazette", "version": "0.1.0", "license": "AGPL-3.0-or-later"})
        self.assertTrue(bare.startswith("Gazette\n\nThis repository"))

    def test_held_root_notice_matches_the_configuration(self):
        root = Path(__file__).resolve().parents[2]
        config = distribution.tomllib.load((root / "tools/distribution-evidence.toml").open("rb"))
        self.assertEqual((root / "NOTICE").read_text(), distribution.notice_text(config["product"]))

if __name__ == "__main__":
    unittest.main()
