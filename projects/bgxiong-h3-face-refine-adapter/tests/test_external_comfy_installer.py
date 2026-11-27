import importlib.util
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "face_node_installer", Path(__file__).resolve().parents[1] / "install-comfyui-nodes.py"
)
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class ComfyInstallerTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.sandbox = Path(temporary.name)
        self.package = self.sandbox / "extracted"
        self.source = self.package / "comfy-nodes"
        self.comfy = self.sandbox / "ComfyUI"
        self.target = self.comfy / "custom_nodes"
        self.target.mkdir(parents=True)
        (self.comfy / "main.py").touch()
        (self.comfy / "folder_paths.py").touch()
        (self.package / "package").mkdir(parents=True)
        manifest = self.package / "package" / "manifest.json"
        manifest.write_text(
            json.dumps({"pluginId": "local.thirdparty.h3-face-refine"}), encoding="utf8"
        )
        for name in installer.NODE_NAMES:
            component = self.source / name
            component.mkdir(parents=True)
            (component / "__init__.py").write_bytes(name.encode())
        wheel = self.source / installer.SCENE_WHEEL
        wheel.parent.mkdir()
        wheel.write_bytes(b"test dependency")
        self.envelope_path = self.package / "adapter-install.json"
        self.envelope = {
            "manifestSha256": installer.digest(manifest),
            "comfyui": {"customNodes": "comfy-nodes", "files": [
                {"path": path.relative_to(self.package).as_posix(), "sha256": installer.digest(path)}
                for path in self.source.rglob("*") if path.is_file()
            ]},
        }
        self.write_envelope()

    def write_envelope(self):
        self.envelope_path.write_text(json.dumps(self.envelope), encoding="utf8")

    def test_exact_bytes_and_repeat_install_preserves_old_and_other_nodes(self):
        old, second = installer.NODE_NAMES
        previous = self.target / old
        previous.mkdir()
        (previous / "local.txt").write_text("previous", encoding="utf8")
        other = self.target / "Unrelated"
        other.mkdir()
        (other / "local.txt").touch()
        inventory = installer.package_inventory(self.package)
        nodes = installer.node_inventories(inventory)
        target = installer.resolve_target(self.comfy, None)
        self.assertEqual(installer.installed(target, nodes), {old: "replace", second: "new"})
        backup = installer.install(self.source, target, nodes)
        self.assertEqual((backup / old / "local.txt").read_text(encoding="utf8"), "previous")
        self.assertTrue((other / "local.txt").is_file())
        cache = previous / "__pycache__"
        cache.mkdir()
        (cache / "module.pyc").touch()
        self.assertEqual(installer.installed(target, nodes), {old: "current", second: "current"})
        self.assertIsNone(installer.install(self.source, target, nodes))
        self.assertEqual(list(path for path in backup.parent.iterdir() if path.is_dir()), [backup])

    def test_tampering_and_undeclared_bytes_are_rejected(self):
        component = self.source / installer.NODE_NAMES[0]
        extra = component / "unexpected.py"
        extra.touch()
        with self.assertRaisesRegex(ValueError, "unexpected.py"):
            installer.package_inventory(self.package)
        extra.unlink()
        (component / "__init__.py").write_text("altered", encoding="utf8")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            installer.package_inventory(self.package)

    def test_rejects_case_duplicates_and_path_traversal(self):
        files = self.envelope["comfyui"]["files"]
        files.append({**files[0], "path": files[0]["path"].replace("__init__", "__INIT__")})
        self.write_envelope()
        with self.assertRaisesRegex(ValueError, "重复"):
            installer.package_inventory(self.package)
        files[-1]["path"] = files[0]["path"].replace("__init__.py", "../escape.py")
        self.write_envelope()
        with self.assertRaisesRegex(ValueError, "越界"):
            installer.package_inventory(self.package)

    def test_failed_swap_restores_both_original_nodes(self):
        old, second = installer.NODE_NAMES
        for name in installer.NODE_NAMES:
            previous = self.target / name
            previous.mkdir()
            (previous / "old.txt").write_text(name, encoding="utf8")
        nodes = installer.node_inventories(installer.package_inventory(self.package))
        rename = Path.rename

        def fail_second_activation(source, target):
            if source.parent.name.startswith("stage-") and target == self.target / second:
                raise OSError("simulated activation failure")
            return rename(source, target)

        with patch.object(Path, "rename", fail_second_activation):
            with self.assertRaisesRegex(OSError, "simulated activation failure"):
                installer.install(self.source, self.target, nodes)
        for name in (old, second):
            self.assertEqual((self.target / name / "old.txt").read_text(encoding="utf8"), name)
            self.assertFalse((self.target / name / "__init__.py").exists())

    def test_session_lock_releases_and_excludes_another_installer(self):
        lock_file = self.target.parent / "install.lock"
        with installer.target_install_lock(lock_file):
            with self.assertRaisesRegex(ValueError, "另一个安装进程"):
                with installer.target_install_lock(lock_file):
                    self.fail("second installation acquired the lock")
        with installer.target_install_lock(lock_file):
            self.assertTrue(lock_file.is_file())

    def test_service_verify_requires_registered_nodes(self):
        ready = {
            "/bgx_face_refine/health": {
                "queuedAnalysis": True, "analysisSessions": True, "trackingReceipt": True,
            },
            **{"/object_info/" + name: {name: {}}
               for name in ("BGXFaceAnalyze", "BGXFaceTrack", "H3FaceSelect")},
        }

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                payload = ready.get(self.path)
                if payload is None:
                    self.send_error(404)
                else:
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(json.dumps(payload).encode())

            def log_message(self, *args):
                pass

        with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                base = f"http://127.0.0.1:{server.server_port}"
                installer.verify_service(base + "/")
                ready["/object_info/BGXFaceAnalyze"] = {}
                with self.assertRaisesRegex(ValueError, "BGXFaceAnalyze"):
                    installer.verify_service(base)
                del ready["/bgx_face_refine/health"]
                with self.assertRaisesRegex(ValueError, "HTTP 404"):
                    installer.verify_service(base)
            finally:
                server.shutdown()
                thread.join()


if __name__ == "__main__":
    unittest.main()
