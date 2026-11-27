import hashlib
import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

path = Path(__file__).parent / "custom_nodes/BGXFaceRefineContract/seed.py"
spec = importlib.util.spec_from_file_location("seed", path)
seed = importlib.util.module_from_spec(spec)
spec.loader.exec_module(seed)


class TestSeed(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.files = {}
        self.samples = []
        for n in range(5):
            key = f"frame-{n}"
            data = key.encode()
            file = Path(tmp.name) / key
            file.write_bytes(data)
            self.files[key] = file
            self.samples.append(dict(mediaId=key, uploadName=key, width=100, height=50,
                                     imageDigest="sha256:"+hashlib.sha256(data).hexdigest()))

    def detect(self, hit):
        calls = []
        def predict(frame, **_):
            calls.append(frame.id)
            boxes = [[10, 5, 55, 27]] if frame.id == hit else []
            return [SimpleNamespace(boxes=SimpleNamespace(xyxy=SimpleNamespace(tolist=lambda: boxes)))]
        detector = SimpleNamespace(predict=predict)
        result = seed.detect_samples(self.samples, detector, .35,
                                     self.files.__getitem__,
                                     lambda path: SimpleNamespace(shape=(50,100,3), id=path.name))
        return calls, result

    def test_current_match_stops_after_one(self):
        calls, result = self.detect("frame-0")
        self.assertEqual(calls, ["frame-0"])
        self.assertEqual(result["mediaId"], "frame-0")
        self.assertEqual(result["candidates"][0]["box"]["x"], .1)

    def test_neighbor_reports_actual_frame(self):
        calls, result = self.detect("frame-2")
        self.assertEqual(calls, ["frame-0", "frame-1", "frame-2"])
        self.assertEqual(result["imageDigest"], self.samples[2]["imageDigest"])

    def test_empty_neighborhood_returns_original_frame(self):
        calls, result = self.detect(None)
        self.assertEqual(len(calls), 5)
        self.assertEqual(result["mediaId"], "frame-0")
        self.assertEqual(result["candidates"], [])

    def test_modified_frame_rejected_before_inference(self):
        self.files["frame-0"].write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            self.detect("frame-0")
