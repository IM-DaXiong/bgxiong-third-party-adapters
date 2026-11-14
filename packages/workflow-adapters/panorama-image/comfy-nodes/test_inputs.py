"""Lightweight bridge tests. Comfy/GPU execution is separately accepted on the resource host."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import types
import unittest


class Video:
    def __init__(self, path): self.path = path
    def get_dimensions(self): return (640, 320)
    def get_frame_rate(self): return 29.97
    def get_frame_count(self): return 73
    def get_duration(self): return 73 / 29.97


class Inputs(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.saved = sys.modules.copy()
        sys.modules["folder_paths"] = types.SimpleNamespace(get_input_directory=lambda: str(self.root))
        sys.modules["comfy_api.latest"] = types.SimpleNamespace(
            IO=types.SimpleNamespace(ComfyNode=object, NodeOutput=lambda *values: values),
            ComfyExtension=object, InputImpl=types.SimpleNamespace(VideoFromFile=Video),
            Types=types.SimpleNamespace(File3D=lambda value: ("file3d", value)))
        sys.modules["aiohttp"] = types.SimpleNamespace(web=types.SimpleNamespace())
        sys.modules["server"] = types.SimpleNamespace(PromptServer=types.SimpleNamespace(instance=types.SimpleNamespace(
            routes=types.SimpleNamespace(post=lambda path: lambda handler: handler))))
        spec = importlib.util.spec_from_file_location("bridge", Path(__file__).parent / "bgx_panorama_inputs" / "__init__.py")
        self.bridge = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.bridge)

    def tearDown(self):
        for key in set(sys.modules) - set(self.saved): del sys.modules[key]
        sys.modules.update(self.saved)
        self.temp.cleanup()

    def test_owned_file_loader_and_native_video_metadata(self):
        (self.root / "owned").mkdir()
        path = self.root / "owned" / "walk.mp4"
        path.write_bytes(b"fixture")
        native = self.bridge.BGXPanoramaVideo.execute("owned/walk.mp4")[0]
        self.assertEqual(native.path, str(path))
        metadata = self.bridge.input_preview({"file":"owned/walk.mp4", "role":"background", "stateJson":"{}", "stickerState":""})
        self.assertEqual(metadata["video"]["frames"], 73)
        self.assertEqual(metadata["video"]["fps"], 29.97)
        self.assertEqual(metadata["video"]["width"], 640)
        for bad in ("../walk.mp4", "owned/../owned/walk.mp4", str(path), "C:/walk.mp4", "owned\\walk.mp4"):
            with self.assertRaises((ValueError, FileNotFoundError)): self.bridge.BGXPanoramaVideo.execute(bad)
        with self.assertRaises(ValueError): self.bridge.input_preview({"file":"owned/walk.mp4", "role":"world", "stateJson":"{}", "stickerState":""})

    def test_pose_uses_loaded_upstream_and_keeps_original_stored_lanes(self):
        path = self.root / "sticker.png";path.write_bytes(b"fixture")
        stored = {"id":"port", "source":{"kind":"port"}, "placement":"sky", "direction":{"value":"stored"}}
        scene = {"timeline":{"fps":29.97}, "next_sticker_id":5, "stickers":[stored]}
        calls = []
        sys.modules["installed.comfyui_pano_suite.core.scene_state"] = types.SimpleNamespace(
            load_scene_state=lambda text: scene, load_sticker_state=lambda text: {"original":text})
        sys.modules["installed.comfyui_pano_suite.port_sticker"] = types.SimpleNamespace(
            _handoff_lanes=lambda pose,fps: calls.append((pose,fps)) or {"direction":"from upstream"})
        result=self.bridge.input_preview({"file":"sticker.png","role":"sticker","stateJson":"{}","stickerState":"original JSON"})
        self.assertEqual(calls,[({"original":"original JSON"},29.97)])
        self.assertIs(result["sticker"],stored)
        self.assertEqual(result["handoff"]["lanes"],{"direction":"from upstream"})
        sys.modules["second.comfyui_pano_suite.port_sticker"] = types.SimpleNamespace()
        with self.assertRaisesRegex(ValueError,"Exactly one"): self.bridge.upstream_module("port_sticker")


if __name__ == "__main__": unittest.main()
