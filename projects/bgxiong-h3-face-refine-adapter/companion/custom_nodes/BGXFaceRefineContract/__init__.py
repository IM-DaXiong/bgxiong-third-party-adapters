"""Package-owned face tracking receipt and version-scoped analysis sessions."""
from pathlib import Path
import folder_paths

from .tracking import NODE_CLASS_MAPPINGS, NODE_DISPLAY_NAME_MAPPINGS
from .face_track import BGXFaceTrack
from .session_api import register

# Expose the bundled workflow's detector paths even in a standalone Comfy install.
folder_paths.add_model_folder_path("ultralytics", str(Path(folder_paths.models_dir) / "ultralytics"))
folder_paths.folder_names_and_paths["ultralytics"][1].add(".pt")
folder_paths.filename_list_cache.pop("ultralytics", None)

register()
NODE_CLASS_MAPPINGS["BGXFaceTrack"] = BGXFaceTrack
NODE_DISPLAY_NAME_MAPPINGS["BGXFaceTrack"] = "人脸跟踪与轨迹复用"
