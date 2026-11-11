"""Bounded Cargo-JSON/file controls for T-T7.3.1 evidence; no Cargo or native calls."""

import copy
import os
from pathlib import Path
import tempfile
import unittest

from build import fixture_artifact
from evidence import InvalidEvidence, sha256


class ArtifactProvenanceControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.repo, self.target = self.root / "source", self.root / "target"
        self.leaf = self.repo / "crates/hex-cli/tests/completion_backends.rs"
        self.manifest = self.repo / "crates/hex-cli/Cargo.toml"
        self.leaf.parent.mkdir(parents=True)
        self.target.mkdir()
        self.leaf.write_bytes(b"current fixture source")
        self.manifest.write_bytes(b"current package manifest")
        self.binary = self.target / "completion_backends"
        self.binary.write_bytes(b"data-only artifact, never executable")
        self.before = {path.relative_to(self.repo).as_posix(): sha256(path)
                       for path in (self.leaf, self.manifest)}
        self.row = {"reason": "compiler-artifact", "fresh": False,
                    "manifest_path": str(self.manifest),
                    "target": {"name": "completion_backends", "kind": ["test"],
                               "crate_types": ["bin"], "src_path": str(self.leaf)},
                    "profile": {"test": True}, "filenames": [str(self.binary)],
                    "executable": str(self.binary)}

    def accept(self, row=None):
        return fixture_artifact(self.row if row is None else row, self.repo, self.target,
                                "hex-cli", "completion_backends", self.before)

    def test_current_compilation_retains_source_and_target_join(self):
        executable, provenance = self.accept()
        self.assertEqual(executable, self.binary)
        self.assertEqual(provenance, {"fresh": False,
            "manifest_path": "crates/hex-cli/Cargo.toml",
            "src_path": "crates/hex-cli/tests/completion_backends.rs",
            "executable": "completion_backends"})

    def test_cached_artifact_refuses_even_with_current_source_manifest(self):
        original_source, original_binary = sha256(self.leaf), sha256(self.binary)
        original_stat = self.leaf.stat()
        self.leaf.write_bytes(b"changed fixture with required drives field")
        os.utime(self.leaf, ns=(original_stat.st_atime_ns, original_stat.st_mtime_ns))
        self.before[self.leaf.relative_to(self.repo).as_posix()] = sha256(self.leaf)
        self.assertNotEqual(sha256(self.leaf), original_source)
        self.assertEqual(self.leaf.stat().st_mtime_ns, original_stat.st_mtime_ns)
        self.assertEqual(sha256(self.binary), original_binary)
        for value in (True, None, 0):
            with self.subTest(fresh=value), self.assertRaisesRegex(InvalidEvidence, "cached fixture"):
                self.accept(dict(self.row, fresh=value))
        # Equal binaries alone cannot reject a real current compilation.
        self.assertEqual(self.accept()[0], self.binary)

    def test_old_source_or_manifest_path_refuses_even_with_equal_bytes(self):
        for field, current in (("manifest_path", self.manifest), ("src_path", self.leaf)):
            foreign = self.root / "old-source" / current.relative_to(self.repo)
            foreign.parent.mkdir(parents=True, exist_ok=True)
            foreign.write_bytes(current.read_bytes())
            row = copy.deepcopy(self.row)
            (row if field == "manifest_path" else row["target"])[field] = str(foreign)
            with self.subTest(field=field), self.assertRaisesRegex(InvalidEvidence, "another source"):
                self.accept(row)

    def test_changed_current_input_refuses_before_binary_copy(self):
        for current in (self.manifest, self.leaf):
            original = current.read_bytes()
            current.write_bytes(original + b"changed after pin")
            with self.subTest(path=current.name), self.assertRaisesRegex(InvalidEvidence, "pinned source"):
                self.accept()
            current.write_bytes(original)

    def test_wrong_target_profile_or_generated_file_refuses(self):
        mutations = (("target", "name", "other_fixture"), ("target", "kind", ["bin"]),
                     ("target", "crate_types", ["lib"]), ("profile", "test", False),
                     ("profile", "test", 1))
        for section, field, value in mutations:
            row = copy.deepcopy(self.row)
            row[section][field] = value
            with self.subTest(section=section, field=field, value=value), self.assertRaises(InvalidEvidence):
                self.accept(row)
        with self.assertRaisesRegex(InvalidEvidence, "generated files"):
            self.accept(dict(self.row, filenames=[]))

    def test_foreign_target_and_symlink_escape_refuse(self):
        foreign = self.root / "old-target-binary"
        foreign.write_bytes(self.binary.read_bytes())
        link = self.target / "escaped"
        link.symlink_to(foreign)
        for path in (foreign, link):
            row = dict(self.row, executable=str(path), filenames=[str(path)])
            with self.subTest(path=path.name), self.assertRaisesRegex(InvalidEvidence, "private target"):
                self.accept(row)


if __name__ == "__main__":
    unittest.main()
