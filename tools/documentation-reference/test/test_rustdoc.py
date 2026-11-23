"""Cargo identity and source-preservation checks for the configured provider."""

import importlib.util
import json
from pathlib import Path
import tempfile
import subprocess
import sys
from types import SimpleNamespace
import unittest

SPEC = importlib.util.spec_from_file_location("rustdoc", Path(__file__).parents[1] / "rustdoc.py")
PROVIDER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROVIDER)


class Preparation(unittest.TestCase):
    def test_renamed_dependencies_and_versions_follow_cargo_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory).resolve()
            old = target / "libsame-old.rmeta"
            new = target / "libsame-new.rmeta"
            stale = target / "libsame-stale.rmeta"
            for path in (old, new, stale):
                path.write_bytes(path.name.encode())
            packages = [{"id": f"registry+https://github.com/rust-lang/crates.io-index#same@{version}", "name": "same", "version": version, "source": next(iter(PROVIDER.CRATES_IO))} for version in ("1.2.3", "2.0.1")]
            # Cargo's dependency alias does not rename the package or artifact crate.
            metadata = {"packages": packages, "resolve": {"nodes": [{"deps": [{"name": "old_alias", "pkg": packages[0]["id"]}, {"name": "new_alias", "pkg": packages[1]["id"]}]}]}}
            messages = [{"reason": "compiler-artifact", "package_id": package["id"], "target": {"name": "same", "kind": ["lib"]}, "filenames": [str(path)]} for package, path in zip(packages, (old, new))]
            found = PROVIDER.dependencies(metadata, messages, target, "root", "a" * 64, {})
            self.assertEqual([item["version"] for item in found], ["1.2.3", "2.0.1"])
            self.assertEqual({item["artifact"] for item in found}, {str(old), str(new)})
            self.assertEqual(len({item["alias"] for item in found}), 2)
            self.assertEqual([item["root"] for item in found], ["https://docs.rs/same/1.2.3/", "https://docs.rs/same/2.0.1/"])
            self.assertEqual(found, PROVIDER.dependencies(metadata, list(reversed(messages)), target, "root", "a" * 64, {}))

    def test_aliases_do_not_follow_unreproducible_metadata_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory).resolve()
            packages = [{"id": f"registry+https://github.com/rust-lang/crates.io-index#{name}@1.0.0", "name": name, "version": "1.0.0", "source": next(iter(PROVIDER.CRATES_IO))} for name in ("first", "second")]
            paths = [target / f"lib{package['name']}-0123abcd.rmeta" for package in packages]
            messages = [{"reason": "compiler-artifact", "package_id": package["id"], "target": {"name": package["name"], "kind": ["lib"]}, "filenames": [str(path)]} for package, path in zip(packages, paths)]

            def build(contents):
                for path, value in zip(paths, contents):
                    path.write_bytes(value)
                return PROVIDER.dependencies({"packages": packages}, messages, target, "root", "a" * 64, {})

            # A rebuild may write different metadata bytes under Cargo's same file names.
            before, after = build([b"one", b"two"]), build([b"three", b"zero"])
            self.assertEqual([item["alias"] for item in before], [item["alias"] for item in after])
            self.assertNotEqual([item["artifact_sha256"] for item in before], [item["artifact_sha256"] for item in after])

    def test_preparation_preserves_every_original_byte_and_refuses_shadowing(self):
        source = b'//! Example:\n//! ```\n//! assert_eq!(library::one(), 1);\n//! ```\n/// A value.\npub const fn one() -> usize { 1 }\n'
        artifact = {"alias": "gazette_doc_example"}
        prepared = PROVIDER.prepare(source, [artifact])
        self.assertTrue(prepared.startswith(source))
        self.assertEqual(prepared[len(source):], b'\n#[cfg(doc)]\n#[doc(hidden)]\n#[allow(unused_extern_crates)]\nextern crate gazette_doc_example;\n')
        with self.assertRaises(PROVIDER.Refusal):
            PROVIDER.prepare(source + b'pub struct gazette_doc_example;\n', [artifact])
        self.assertEqual(PROVIDER.prepare(source, []), source)

    def test_ambiguous_package_version_requires_complete_cargo_identity(self):
        packages = [{"id": f"{origin}#same@1.0.0", "name": "same", "version": "1.0.0", "source": origin} for origin in ("git+https://example.invalid/a", "git+https://example.invalid/b")]
        with self.assertRaises(PROVIDER.Refusal):
            PROVIDER.roots({"packages": packages}, {"same@1.0.0": "https://example.invalid/docs/"})
        self.assertEqual(PROVIDER.roots({"packages": packages}, {packages[0]["id"]: "https://example.invalid/docs/"}), {packages[0]["id"]: "https://example.invalid/docs/"})

    def test_artifact_and_local_dependency_paths_cannot_escape_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            target.mkdir()
            artifact = root / "outside.rmeta"
            artifact.write_bytes(b"outside")
            metadata = {"packages": [{"id": "dep", "name": "dep", "version": "1.0.0", "source": next(iter(PROVIDER.CRATES_IO))}]}
            message = {"reason": "compiler-artifact", "package_id": "dep", "target": {"name": "dep", "kind": ["lib"]}, "filenames": [str(artifact)]}
            with self.assertRaises(PROVIDER.Refusal):
                PROVIDER.dependencies(metadata, [message], target, "root", "a" * 64, {})
            (target / "link").symlink_to(artifact)
            with self.assertRaises(PROVIDER.Refusal):
                PROVIDER.tree(target)

    def test_explicit_tool_flags_and_legacy_json_select_the_same_observed_tools(self):
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "selected-tool"
            executable.write_bytes(b"selected executable")
            config = {name: str(executable) for name in ("cargo", "rustc", "rustdoc")}
            arguments = SimpleNamespace(**{name: [value] for name, value in config.items()})
            legacy = PROVIDER.selected_tools(config, SimpleNamespace())
            self.assertEqual(PROVIDER.selected_tools({}, arguments), legacy)
            self.assertEqual(PROVIDER.selected_tools(config, arguments), legacy)
            self.assertEqual(set(legacy), {"cargo", "rustc", "rustdoc"})

    def test_missing_relative_and_conflicting_tool_selections_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "selected-tool"
            executable.write_bytes(b"selected executable")
            config = {name: str(executable) for name in ("cargo", "rustc", "rustdoc")}
            for name in config:
                missing = {key: value for key, value in config.items() if key != name}
                with self.assertRaises(PROVIDER.Refusal):
                    PROVIDER.selected_tools(missing, SimpleNamespace())
                for value in (None, "", "relative/tool"):
                    with self.assertRaises(PROVIDER.Refusal):
                        PROVIDER.selected_tools(config | {name: value}, SimpleNamespace())
                with self.assertRaisesRegex(PROVIDER.Refusal, "conflicting"):
                    PROVIDER.selected_tools(config, SimpleNamespace(**{name: [str(executable) + "-other"]}))
                with self.assertRaisesRegex(PROVIDER.Refusal, "conflicting"):
                    PROVIDER.selected_tools(missing, SimpleNamespace(**{name: [str(executable), str(executable) + "-other"]}))

    def test_conflicting_cli_tool_paths_fail_before_preparation_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, output = root / "input", root / "output"
            source.mkdir()
            output.mkdir()
            config = {"manifest": "Cargo.toml", **{name: sys.executable for name in ("cargo", "rustc", "rustdoc")}}
            (source / "rustdoc.json").write_text(json.dumps(config))
            for flags in (["--cargo", sys.executable + "-other"], ["--cargo", sys.executable, "--cargo", sys.executable + "-other"]):
                result = subprocess.run([sys.executable, "-B", str(Path(PROVIDER.__file__)), "--input", str(source), "--output", str(output), "--config", "rustdoc.json", *flags], capture_output=True, text=True, check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("conflicting cargo", result.stderr)
                self.assertEqual(list(output.iterdir()), [])

    def test_workspace_only_manifest_is_not_an_implicit_package_selection(self):
        with self.assertRaises(PROVIDER.Refusal):
            PROVIDER.library({"packages": []}, Path("/workspace/Cargo.toml"))


if __name__ == "__main__":
    unittest.main()
