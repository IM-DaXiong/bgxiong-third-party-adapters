import asyncio
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import Mock, patch
from uuid import uuid4

COMPANION = Path(__file__).parent


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    with patch.dict(sys.modules, {name: module}):
        spec.loader.exec_module(module)
    return module


class TestQueuedAnalysis(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.video = self.root / "video.mp4"
        self.video.write_bytes(b"isolated placeholder")
        self.before_node = Mock()
        self.modules = {
            "face_package": ModuleType("face_package"),
            "face_package.face_track": SimpleNamespace(_input_file=Mock(return_value=self.video), _upstream=Mock()),
            "face_package.trajectory": SimpleNamespace(_cut_detector=Mock()),
            "folder_paths": SimpleNamespace(get_temp_directory=self.root.__str__),
            "nodes": SimpleNamespace(before_node_execution=self.before_node),
        }
        self.patch = patch.dict(sys.modules, self.modules)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        self.node = load_module(
            "face_package.queued_analysis",
            COMPANION / "custom_nodes/BGXFaceRefineContract/queued_analysis.py",
        )
        self.request_id = uuid4()

    def test_mapped_result_is_immutable_and_omits_machine_path(self):
        scan = Mock(return_value={"frames": 4, "fps": 24, "width": 1080, "height": 720, "shots": [],
                                  "path": str(self.video), "cached": False})
        with patch.object(self.node, "scan_function", return_value=scan):
            response = self.node.BGXFaceAnalyze().analyze(
                self.video.name, "bbox/face.pt", 0.35, "auto", 3.0, "largest_face", str(self.request_id)
            )
            self.assertEqual(response["ui"]["bgx_analysis"], [self.request_id.hex])
            self.assertEqual(scan.call_args.args[0], str(self.video))
            self.assertIs(scan.call_args.args[2], self.modules["face_package.trajectory"]._cut_detector)
            self.assertIs(scan.call_args.args[3], self.before_node)
            destination = self.node.artifact_path(self.request_id.hex)
            self.assertEqual(json.loads(destination.read_bytes()),
                             {"frames": 4, "fps": 24, "width": 1080, "height": 720, "shots": []})
            scan.return_value["frames"] = 8
            with self.assertRaisesRegex(ValueError, "already belongs"):
                self.node.BGXFaceAnalyze().analyze(
                    self.video.name, "bbox/face.pt", 0.35, "none", 3.0, "largest_face", str(self.request_id)
                )
            self.assertEqual(json.loads(destination.read_bytes())["frames"], 4)

    def test_invalid_request_and_model_paths_fail_before_scan(self):
        with patch.object(self.node, "scan_function") as scan:
            for owner, model in [("../outside", "bbox/face.pt"), (str(self.request_id), "../outside.pt")]:
                with self.assertRaises(ValueError):
                    self.node.BGXFaceAnalyze().analyze(self.video.name, model, 0.35, "auto", 3.0, "largest_face", owner)
            scan.assert_not_called()

    def test_cancel_requires_own_prompt_and_own_request(self):
        request_id = self.request_id.hex
        other = uuid4().hex
        pending = [
            (0, "p1", {"1": {"class_type": "BGXFaceAnalyze", "inputs": {"request_id": other}}}),
            (0, "p3", {"1": {"class_type": "AnotherNode", "inputs": {"request_id": request_id}}}),
            (0, "p2", {"1": {"class_type": "BGXFaceAnalyze", "inputs": {"request_id": str(self.request_id)}}}),
        ]

        def delete(predicate):
            selected = next((item for item in pending if predicate(item)), None)
            if selected is None:
                return False
            pending.remove(selected)
            return True

        queue = SimpleNamespace(delete_queue_item=delete)
        self.assertFalse(self.node.cancel_pending(queue, "p1", request_id))
        self.assertFalse(self.node.cancel_pending(queue, "p3", request_id))
        self.assertTrue(self.node.cancel_pending(queue, "p2", request_id))
        self.assertFalse(self.node.cancel_pending(queue, "p2", request_id))
        self.assertEqual([item[1] for item in pending], ["p1", "p3"])

    def test_legacy_busy_guard_and_new_queue_path_use_one_scanner(self):
        module = ModuleType("upstream_test")
        module.nodes = SimpleNamespace(
            _resolve_video=Mock(return_value=str(self.video)),
            _make_cut_detector=Mock(),
        )
        path = COMPANION / "upstream/ComfyUI-H3-FaceRefine/picker_api.py"
        with patch.dict(sys.modules, {"upstream_test": module, "torch": SimpleNamespace(), "numpy": SimpleNamespace()}):
            picker = load_module("upstream_test.picker_api", path)
        with patch.object(picker, "_render_running", return_value=True) as running, patch.object(picker, "_scan_video") as scan:
            with self.assertRaises(picker._RenderBusy):
                picker._scan({"video": self.video.name})
            scan.assert_not_called()
            interrupt = Mock()
            cut_factory = Mock()
            picker.run_queued_scan(str(self.video), {"video": self.video.name}, cut_factory, interrupt)
            running.assert_called_once_with()
            interrupt.assert_called_once_with()
            self.assertEqual(scan.call_args.args[0], str(self.video))
            self.assertEqual(scan.call_args.args[3:], (cut_factory, None, interrupt))
            self.assertEqual(scan.call_args.kwargs, {"include_previews": False, "strict_cuts": True})

    def test_duplicate_upstream_keeps_one_shared_scanner_and_route_set(self):
        from aiohttp import web

        upstream = ModuleType("upstream_test")
        upstream.nodes = SimpleNamespace()
        routes = web.RouteTableDef()
        instance = SimpleNamespace(routes=routes)
        module_stubs = {"upstream_test": upstream, "torch": SimpleNamespace(), "numpy": SimpleNamespace(),
                        "server": SimpleNamespace(PromptServer=SimpleNamespace(instance=instance))}
        path = COMPANION / "upstream/ComfyUI-H3-FaceRefine/picker_api.py"
        with patch.dict(sys.modules, module_stubs):
            first = load_module("upstream_test.picker_api", path)
            second = load_module("upstream_test.other_picker_api", path)
            self.assertFalse(self.node.available())
            first.register()
            count = len(routes)
            second.register()
            self.assertTrue(self.node.available())
            self.assertIs(self.node.scan_function(), first.run_queued_scan)
            self.assertEqual(len(routes), count)

    def test_service_routes_distinguish_invalid_id_pending_and_unknown(self):
        from aiohttp import web

        routes = web.RouteTableDef()
        queue = SimpleNamespace(delete_queue_item=Mock(return_value=False))
        self.modules["folder_paths"].get_temp_directory = self.root.__str__
        extra = {
            "server": SimpleNamespace(PromptServer=SimpleNamespace(instance=SimpleNamespace(routes=routes, prompt_queue=queue))),
            "face_package.queued_analysis": self.node,
            "face_package.seed": SimpleNamespace(),
            "face_package.sessions": SimpleNamespace(save_session=Mock(), load_session=Mock()),
        }
        with patch.dict(sys.modules, extra):
            service = load_module("face_package.session_api",
                COMPANION / "custom_nodes/BGXFaceRefineContract/session_api.py")
            service.register()
            initial_count = len(routes)
            duplicate = load_module("face_package.duplicate_session_api",
                COMPANION / "custom_nodes/BGXFaceRefineContract/session_api.py")
            duplicate.register()
            self.assertEqual(len(routes), initial_count)
        handlers = {item.path: item.handler for item in routes}
        target = "/bgx_face_refine/queued-analysis/{request_id}"

        async def cancel():
            return {"promptId": "owned"}

        health = handlers["/bgx_face_refine/health"]
        with patch.object(self.node, "available", return_value=True):
            response = asyncio.run(health(None))
        self.assertTrue(json.loads(response.body)["queuedAnalysis"])
        response = asyncio.run(handlers[target](SimpleNamespace(match_info={"request_id": "invalid"})))
        self.assertEqual(response.status, 400)
        response = asyncio.run(handlers[target](SimpleNamespace(match_info={"request_id": self.request_id.hex})))
        self.assertEqual(response.status, 404)
        request = SimpleNamespace(match_info={"request_id": self.request_id.hex}, json=cancel)
        response = asyncio.run(handlers[f"{target}/cancel"](request))
        self.assertEqual(json.loads(response.body), {"outcome": "unsupported"})
        queue.delete_queue_item.return_value = True
        response = asyncio.run(handlers[f"{target}/cancel"](request))
        self.assertEqual(json.loads(response.body), {"outcome": "remote_confirmed"})


if __name__ == "__main__":
    unittest.main()
