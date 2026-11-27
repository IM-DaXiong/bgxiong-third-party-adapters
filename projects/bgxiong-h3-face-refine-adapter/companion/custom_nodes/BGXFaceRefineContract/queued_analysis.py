"""ComfyUI queue node and result ownership for face target analysis."""

import json
import os
import tempfile
import uuid
from pathlib import Path, PurePosixPath

import folder_paths

from .face_track import _input_file
from .trajectory import _cut_detector

CLASS_NAME = "BGXFaceAnalyze"


def analysis_id(request_id):
    try:
        return uuid.UUID(request_id).hex
    except (ValueError, TypeError, AttributeError) as error:
        raise ValueError("Invalid queued face analysis request ID") from error


def artifact_path(request_id):
    return Path(folder_paths.get_temp_directory()) / "bgx-face-refine" / "queued-analysis-v1" / (
        analysis_id(request_id) + ".json"
    )


def scan_function():
    from server import PromptServer

    return getattr(PromptServer.instance, "_bgx_h3_queued_scan", None)


def available():
    return callable(scan_function())


def cancel_pending(queue, prompt_id, request_id):
    def matches(item):
        if item[1] != prompt_id or set(item[2]) != {"1"}:
            return False
        node = item[2]["1"]
        if node["class_type"] != CLASS_NAME:
            return False
        try:
            return analysis_id(node["inputs"].get("request_id")) == request_id
        except ValueError:
            return False

    delete_queued = getattr(queue, "delete_queue_item", None)
    return callable(delete_queued) and delete_queued(matches)


def _save_result(destination, data):
    destination.parent.mkdir(parents=True, exist_ok=True)
    data = {key: value for key, value in data.items() if key not in ("path", "cached")}
    if destination.exists():
        if json.loads(destination.read_bytes()) != data:
            raise ValueError("Queued face analysis request ID already belongs to a different result")
        return
    fd, temporary = tempfile.mkstemp(dir=destination.parent, suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf8") as output:
            json.dump(data, output, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        os.replace(temporary, destination)
    finally:
        Path(temporary).unlink(missing_ok=True)


class BGXFaceAnalyze:
    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "video": ("STRING",),
                "detector": ("STRING", {"default": "bbox/face_yolov8m.pt"}),
                "confidence": ("FLOAT", {"default": 0.35, "min": 0.05, "max": 0.95}),
                "cut_detection": (["none", "auto"],),
                "cut_threshold": ("FLOAT", {"default": 3.0, "min": 0.5, "max": 20.0}),
                "select": (["largest_face", "smallest_face", "left_most", "right_most", "top_most",
                            "bottom_most", "centre_most", "closest_to_xy", "detector_score"],),
                "request_id": ("STRING",),
            },
            "optional": {"force": ("BOOLEAN", {"default": False})},
        }

    RETURN_TYPES = ()
    OUTPUT_NODE = True
    FUNCTION = "analyze"
    CATEGORY = "H3/Face Refine"

    def analyze(self, video, detector, confidence, cut_detection, cut_threshold, select, request_id,
                force=False):
        from nodes import before_node_execution

        destination = artifact_path(request_id)
        source = _input_file(video)
        detector_path = PurePosixPath(detector.replace("\\", "/"))
        if detector_path.is_absolute() or ".." in detector_path.parts or ":" in detector:
            raise ValueError("Invalid face detector model name")
        scan = scan_function()
        if not callable(scan):
            raise ValueError("ComfyUI is missing the face target scan nodes")
        raw = scan(
            str(source),
            {"detector": detector, "confidence": confidence, "cut_detection": cut_detection,
             "cut_threshold": cut_threshold, "select": select, "force": force},
            _cut_detector,
            before_node_execution,
        )
        before_node_execution()
        _save_result(destination, raw)
        return {"ui": {"bgx_analysis": [destination.stem]}}
