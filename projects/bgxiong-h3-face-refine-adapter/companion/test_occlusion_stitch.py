"""Run with the Python environment and import path of the target ComfyUI."""
import importlib.util
from pathlib import Path
import unittest

import torch


def load_upstream():
    source = Path(__file__).parent / "upstream/ComfyUI-H3-FaceRefine/nodes.py"
    spec = importlib.util.spec_from_file_location("test_face_refine_upstream", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class OcclusionStitchTest(unittest.TestCase):
    def test_only_detected_source_frames_are_composited_with_or_without_mask(self):
        upstream = load_upstream()
        original = torch.full((5, 32, 32, 3), 0.2)
        refined = torch.full((4, 32, 32, 3), 0.8)
        transform = {
            "canvas": [32, 32], "src_size": [32, 32], "source": [0, 1, 3, 4],
            "boxes": [[0.0, 0.0, 32.0, 32.0]] * 4,
            "face_rect": [[8.0, 8.0, 16.0, 16.0]] * 4, "detected": [True, False, False, True],
        }
        for mask in (None, torch.ones((4, 32, 32))):
            with self.subTest(external_mask=mask is not None):
                (result,) = upstream.H3FaceStitch().run(
                    original, refined, transform, "face_only", 0, 0, 0, 1,
                    undetected_frames="skip", masks=mask,
                )
                self.assertEqual(result.shape, original.shape)
                torch.testing.assert_close(result[1:4], original[1:4], atol=0, rtol=0)
                self.assertTrue(bool(torch.all(result[[0, 4], 16, 16, :] > original[[0, 4], 16, 16, :])))


if __name__ == "__main__":
    unittest.main()
