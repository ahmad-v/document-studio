"""Pinned private Pandoc pack creation and archive-safety checks."""
import hashlib
import importlib.util
import io
import tarfile
import tempfile
import unittest
from pathlib import Path

MODULE_SPEC = importlib.util.spec_from_file_location(
    "pandoc_engine", Path(__file__).with_name("pandoc_engine.py")
)
PACKER = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(PACKER)


class PandocPackTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.archive = self.root / "pandoc.tar.gz"
        self.binary = b"private pandoc binary fixture"
        with tarfile.open(self.archive, "w:gz") as archive:
            member = tarfile.TarInfo(PACKER.MEMBER)
            member.mode = 0o755
            member.size = len(self.binary)
            archive.addfile(member, io.BytesIO(self.binary))

    def tearDown(self):
        self.temp.cleanup()

    def test_builds_hash_verified_application_relative_engine_pack(self):
        pack = PACKER.create_pack(
            self.archive,
            self.root / "pack",
            self.archive.stat().st_size,
            PACKER.digest(self.archive),
        )
        PACKER.verify_pack(pack, hashlib.sha256(self.binary).hexdigest())
        executable = pack / "engines/pandoc/bin/pandoc"
        self.assertEqual(executable.read_bytes(), self.binary)
        self.assertTrue(executable.stat().st_mode & 0o111)
        self.assertIn("GPL-2.0-or-later", (pack / "engines/manifest.json").read_text())

    def test_rejects_source_hash_mismatch(self):
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            PACKER.verify_archive(self.archive, self.archive.stat().st_size, "0" * 64)

    def test_rejects_symlink_instead_of_expected_regular_executable(self):
        destination = self.root / "binary"
        with tarfile.open(self.archive, "w:gz") as archive:
            member = tarfile.TarInfo(PACKER.MEMBER)
            member.type = tarfile.SYMTYPE
            member.linkname = "../../outside"
            archive.addfile(member)
        with self.assertRaisesRegex(ValueError, "regular file"):
            PACKER.extract_binary(self.archive, destination)
        self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
