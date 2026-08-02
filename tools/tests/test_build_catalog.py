import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools.build_catalog import scan_artifacts


class BuildCatalogTests(unittest.TestCase):
    def test_driver_package_has_published_artifact_and_signature_urls(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "drivers" / "kbd_tca8418"
            package.mkdir(parents=True)
            elf = package / "kbd_tca8418.drv.elf"
            elf.write_bytes(b"driver artifact")
            Path(str(elf) + ".sig").write_bytes(b"x" * 64)
            Path(str(elf) + ".sha256").write_text(
                hashlib.sha256(elf.read_bytes()).hexdigest() + "\n"
            )
            (package / "manifest.json").write_text(json.dumps({
                "type": "driver",
                "id": "com.thistle.drv.kbd_tca8418",
                "name": "TCA8418",
                "entry": elf.name,
                "arch": "esp32s3",
            }))

            entries = scan_artifacts(str(root), "https://example.invalid/apps")

        self.assertEqual(len(entries), 1)
        self.assertEqual(entries[0]["type"], "driver")
        self.assertEqual(
            entries[0]["url"],
            "https://example.invalid/apps/drivers/kbd_tca8418/kbd_tca8418.drv.elf",
        )
        self.assertEqual(
            entries[0]["sig_url"],
            "https://example.invalid/apps/drivers/kbd_tca8418/kbd_tca8418.drv.elf.sig",
        )
        self.assertEqual(entries[0]["sha256"], hashlib.sha256(b"driver artifact").hexdigest())


if __name__ == "__main__":
    unittest.main()
