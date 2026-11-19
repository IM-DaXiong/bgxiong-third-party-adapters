import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

SCRIPT = Path(__file__).with_name("archive-current-resources.py")
spec = importlib.util.spec_from_file_location("archive_current_resources", SCRIPT)
archive = importlib.util.module_from_spec(spec)
spec.loader.exec_module(archive)
HOST = SCRIPT.parents[2] / "bgxiong-ai-story"


class CurrentOwnerArchiveTests(unittest.TestCase):
    def fixture(self, base, crlf=False):
        root = base / "adapters"
        (root / "release").mkdir(parents=True)
        files = []
        for relative, data in (("adapter-install.json", b"envelope"), ("package/adapter.wasm", b"frozen-wasm")):
            name = "artifacts/current/local.test/1.0.0/" + relative
            source = root / name
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_bytes(data)
            files.append({"path": relative, "source": name, "sha256": hashlib.sha256(data).hexdigest()})
        lock = {"schemaVersion": 1, "projectName": "bgxiong-third-party-adapters", "packages": [{"pluginId": "local.test", "version": "1.0.0", "files": files}]}
        raw = (json.dumps(lock, indent=2) + "\n").encode()
        (root / "release/current.json").write_bytes(raw.replace(b"\n", b"\r\n") if crlf else raw)
        (root / "release/current-artifact.json").write_bytes(b"existing-official-receipt")
        output = base / ("third-party-frozen-resources-" + hashlib.sha256(raw).hexdigest()[:12] + ".zip")
        return root, output, base / "candidate.json", lock

    def test_real_owner_restore_and_idempotent_reuse_preserve_official_receipt(self):
        with tempfile.TemporaryDirectory() as temp:
            root, output, receipt, _ = self.fixture(Path(temp))
            first = archive.prepare(root, HOST, output, receipt)
            raw = output.read_bytes()
            second = archive.prepare(root, HOST, output, receipt)
            self.assertEqual(first["isolatedRestore"]["restored"], 2)
            self.assertEqual(first["isolatedSecondRestore"]["alreadyPresent"], 2)
            self.assertFalse(first["uploaded"])
            self.assertTrue(first["doNotClaimDownloadable"])
            self.assertTrue(second["reusedExistingArchive"])
            self.assertEqual(output.read_bytes(), raw)
            self.assertEqual((root / "release/current-artifact.json").read_bytes(), b"existing-official-receipt")

    def test_crlf_checkout_uses_canonical_lf_owner_fingerprint(self):
        with tempfile.TemporaryDirectory() as temp:
            root, output, receipt, _ = self.fixture(Path(temp), crlf=True)
            result = archive.prepare(root, HOST, output, receipt)
            self.assertIn(result["lockSha256"][:12], output.name)
            self.assertEqual(result["isolatedRestore"]["restored"], 2)

    def test_missing_source_leaves_no_archive_or_candidate_receipt(self):
        with tempfile.TemporaryDirectory() as temp:
            root, output, receipt, lock = self.fixture(Path(temp))
            (root / lock["packages"][0]["files"][0]["source"]).unlink()
            with self.assertRaises(FileNotFoundError):
                archive.prepare(root, HOST, output, receipt)
            self.assertFalse(output.exists())
            self.assertFalse(receipt.exists())

    def test_changed_source_leaves_no_archive(self):
        with tempfile.TemporaryDirectory() as temp:
            root, output, receipt, lock = self.fixture(Path(temp))
            (root / lock["packages"][0]["files"][0]["source"]).write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "digest mismatch"):
                archive.prepare(root, HOST, output, receipt)
            self.assertFalse(output.exists())

    def test_wrong_existing_archive_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as temp:
            root, output, receipt, _ = self.fixture(Path(temp))
            with zipfile.ZipFile(output, "x") as zipped:
                zipped.writestr("unexpected", b"keep")
            raw = output.read_bytes()
            with self.assertRaisesRegex(ValueError, "different file set"):
                archive.prepare(root, HOST, output, receipt)
            self.assertEqual(output.read_bytes(), raw)
            self.assertFalse(receipt.exists())

    def test_official_owner_receipt_cannot_be_replaced(self):
        with tempfile.TemporaryDirectory() as temp:
            root, output, _, _ = self.fixture(Path(temp))
            with self.assertRaisesRegex(ValueError, "cannot replace"):
                archive.prepare(root, HOST, output, root / "release/current-artifact.json")
            self.assertEqual((root / "release/current-artifact.json").read_bytes(), b"existing-official-receipt")
            self.assertFalse(output.exists())

    def test_archive_cannot_pollute_unique_installation_tree(self):
        with tempfile.TemporaryDirectory() as temp:
            root, output, receipt, _ = self.fixture(Path(temp))
            with self.assertRaisesRegex(ValueError, "unique installation tree"):
                archive.prepare(root, HOST, root / "artifacts/current" / output.name, receipt)

    def test_wrong_archive_lock_label_rejected_before_write(self):
        with tempfile.TemporaryDirectory() as temp:
            root, _, receipt, _ = self.fixture(Path(temp))
            output = Path(temp) / "third-party-frozen-resources-wrong.zip"
            with self.assertRaisesRegex(ValueError, "canonical LF lock fingerprint"):
                archive.prepare(root, HOST, output, receipt)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
