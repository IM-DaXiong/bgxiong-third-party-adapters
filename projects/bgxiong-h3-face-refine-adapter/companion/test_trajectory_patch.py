import importlib.util
from pathlib import Path
import unittest

path = Path(__file__).parent / "custom_nodes/BGXFaceRefineContract/trajectory.py"
spec = importlib.util.spec_from_file_location("trajectory", path)
trajectory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trajectory)


class TestTrajectory(unittest.TestCase):
    def test_keeps_crop_geometry_and_leaves_gaps_for_lost_frames(self):
        geometry = dict(
            canvas=[512,512], src_size=[640,360], source_frames=5, frames=3,
            source=[0,1,3], boxes=[[20,20,150,150]]*3, face_rect=[[110,120,200,200]]*3,
            weights=[.2, .7, .9], segments=[[0,2],[2,3]], crop_factor=2.5,
            detected=[True,False,True], absent=[False]*3)
        layout, rows = trajectory.normalize(geometry)
        self.assertEqual([f["state"] for f in rows], ["present","lost","lost","present","lost"])
        rows[2] = {"state":"skip", "geometry":None}
        composed = trajectory.restore(layout, rows)
        self.assertEqual(composed["source_frames"], 5)
        self.assertEqual(composed["source"], [0,3])
        self.assertEqual(composed["weights"], [.2,.9])
        self.assertEqual(composed["segments"], [[0,1],[1,2]])
        self.assertEqual(composed["boxes"], [geometry["boxes"][0],geometry["boxes"][2]])

    def test_rejects_invalid_crop_and_unresolved_frames(self):
        layout = dict(canvas=[512,512], sourceSize=[300,200], cropFactor=2.5)
        with self.assertRaisesRegex(ValueError, "Unresolved"):
            trajectory.restore(layout, [{"state":"lost","geometry":{}}])
        with self.assertRaisesRegex(ValueError, "exceeds"):
            trajectory.restore(layout, [{"state":"present","geometry":{
                "crop":[290,0,150,150],"face":[0,0,10,10],"weight":.5,"segmentStart":0}}])
