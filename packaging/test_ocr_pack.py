"""Fast, host-independent checks for the private OCR pack target guard."""
import unittest
from unittest.mock import patch

from build_ocr_pack import validate_native_target


class NativeOcrTargetTests(unittest.TestCase):
    @patch("build_ocr_pack.platform.system", return_value="Linux")
    def test_accepts_matching_native_linux_targets(self, _system):
        validate_native_target("x86_64-unknown-linux-gnu", "x86_64")
        validate_native_target("aarch64-unknown-linux-gnu", "aarch64")

    @patch("build_ocr_pack.platform.system", return_value="Linux")
    def test_rejects_wrong_architecture(self, _system):
        with self.assertRaisesRegex(RuntimeError, "matching native Linux runner"):
            validate_native_target("aarch64-unknown-linux-gnu", "x86_64")

    @patch("build_ocr_pack.platform.system", return_value="Darwin")
    def test_rejects_non_linux_hosts(self, _system):
        with self.assertRaisesRegex(RuntimeError, "matching native Linux runner"):
            validate_native_target("x86_64-unknown-linux-gnu", "x86_64")


if __name__ == "__main__":
    unittest.main()
