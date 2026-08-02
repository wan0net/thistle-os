import json
import tempfile
import unittest
from pathlib import Path

from tools.validate_arch_coverage import validate_firmware, validate_packages


ARCHES = {"esp32", "esp32s3", "esp32c3"}


class ArchitectureCoverageTests(unittest.TestCase):
    def test_firmware_requires_every_advertised_architecture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for arch in ARCHES - {"esp32c3"}:
                path = root / f"thistle-os-firmware-{arch}"
                path.mkdir()
                (path / "thistle_os.bin").write_bytes(b"firmware")
            errors = validate_firmware(ARCHES, root)
        self.assertEqual(len(errors), 1)
        self.assertIn("esp32c3", errors[0])

    def test_package_must_match_its_published_architecture_and_include_sidecars(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "esp32s3" / "drivers" / "kbd"
            package.mkdir(parents=True)
            artifact = package / "kbd.drv.elf"
            artifact.write_bytes(b"driver")
            Path(str(artifact) + ".sig").write_bytes(b"x" * 64)
            Path(str(artifact) + ".sha256").write_text("00\n")
            (package / "manifest.json").write_text(json.dumps({
                "arch": "esp32s3", "entry": artifact.name,
            }))
            self.assertEqual(validate_packages(ARCHES, root), [])

            bad = root / "esp32" / "drivers" / "wrong"
            bad.mkdir(parents=True)
            (bad / "manifest.json").write_text(json.dumps({
                "arch": "esp32s3", "entry": "missing.drv.elf",
            }))
            errors = validate_packages(ARCHES, root)
        self.assertTrue(any("not published under architecture" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
