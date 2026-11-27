"""Shared canonical tracking for explicit preview and refinement."""
import hashlib
import json
import math
import os
from pathlib import Path
import sys
import tempfile
import time

import folder_paths
from .tracking import tracking_receipt


CACHE_VERSION = "1.0.2"
CACHE_MAX_BYTES = 64 * 1024 * 1024
CACHE_MAX_AGE = 24 * 60 * 60


def _file_digest(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def _input_file(name):
    root = Path(folder_paths.get_input_directory()).resolve()
    path = (root / name).resolve(strict=True)
    if not path.is_relative_to(root) or not path.is_file():
        raise ValueError("Invalid face-refine input file")
    return path


def _upstream():
    from nodes import NODE_CLASS_MAPPINGS
    select = NODE_CLASS_MAPPINGS["H3FaceSelect"]
    track = NODE_CLASS_MAPPINGS["H3FaceTrackCrop"]
    return select, track, sys.modules[track.__module__]


def _root():
    return Path(folder_paths.get_temp_directory()) / "bgx-face-refine" / "tracking-v2"


def _cache_key(video, identity_file, settings, analysis, selection, upstream):
    source_digest = _file_digest(video)
    expected = analysis["source"]["contentDigest"]
    if expected not in (source_digest, f"sha256:{source_digest}"):
        raise ValueError("跟踪数据不完整：源视频内容与分析不一致")
    key = {
        "version": CACHE_VERSION,
        "upstream": _file_digest(upstream.__file__),
        "video": source_digest,
        "identity": _file_digest(identity_file) if identity_file else None,
        "settings": settings,
        "analysis": analysis,
        "selection": selection,
    }
    # The model folder is mutable, so identify the detector bytes as well as its name.
    detectors = {}
    for name in {settings["select"]["detector"], settings["track"]["detector"],
                 settings["track"].get("fallback_detector", "none")} - {"none"}:
        normalized = name.replace("\\", "/")
        path = next((path for key in ("ultralytics_bbox", "ultralytics")
                     if (path := folder_paths.get_full_path(key, name))), None)
        if path is None:
            base = Path(folder_paths.models_dir) / "ultralytics"
            path = next((prefix / normalized for prefix in (base / "bbox", base, base / "segm")
                         if (prefix / normalized).is_file()), None)
        if path is None:
            return None
        detectors[name] = _file_digest(path)
    key["detectors"] = detectors
    return hashlib.sha256(json.dumps(key, sort_keys=True).encode()).hexdigest()


def _validate_geometry(transform, shape):
    count, height, width = map(int, shape[:3])
    if transform["source_frames"] != count or transform["src_size"] != [width, height]:
        raise ValueError("跟踪数据不完整：源视频帧或尺寸不一致")
    size = transform["canvas"]
    if len(size) != 2 or any(type(n) is not int or not 0 < n <= 768 for n in size):
        raise ValueError("跟踪数据不完整：裁剪尺寸无效")
    count = len(transform["source"])
    if count == 0 or transform["frames"] != count:
        raise ValueError("跟踪数据不完整：裁剪帧数无效")
    for field in ("boxes", "face_rect", "weights"):
        if len(transform[field]) != count:
            raise ValueError("跟踪数据不完整：裁剪数据帧数不一致")
    for frame, face in zip(transform["boxes"], transform["face_rect"]):
        if any(len(box) != 4 or not all(type(n) in (int, float) and math.isfinite(n) for n in box)
               or box[2] <= 0 or box[3] <= 0 for box in (frame, face)):
            raise ValueError("跟踪数据不完整：裁剪几何无效")
        x, y, w, h = frame
        if x < 0 or y < 0 or x + w > width + 1e-6 or y + h > height + 1e-6:
            raise ValueError("跟踪数据不完整：裁剪超出原帧")
    if any(type(w) not in (int, float) or not 0 <= w <= 1 for w in transform["weights"]):
        raise ValueError("跟踪数据不完整：权重无效")
    if any(len(pair) != 2 or not all(type(n) is int for n in pair)
           or not 0 <= pair[0] < pair[1] <= count for pair in transform["segments"]):
        raise ValueError("跟踪数据不完整：镜头区间无效")


def _cache_read(path):
    try:
        with path.open("rb") as stream:
            if time.time() - os.fstat(stream.fileno()).st_mtime >= CACHE_MAX_AGE:
                return None
            raw = stream.read(CACHE_MAX_BYTES + 1)
            if len(raw) > CACHE_MAX_BYTES:
                return None
            body = json.loads(raw)
        payload = json.dumps(body["trajectory"], sort_keys=True, allow_nan=False)
        if body["key"] == path.stem and hashlib.sha256(payload.encode()).hexdigest() == body["digest"]:
            return body["trajectory"]
    except (OSError, TypeError, ValueError, KeyError):
        pass
    return None


def _cache_write(path, trajectory):
    payload = json.dumps(trajectory, sort_keys=True, allow_nan=False)
    body = json.dumps({"key": path.stem, "trajectory": trajectory,
                       "digest": hashlib.sha256(payload.encode()).hexdigest()}).encode()
    if len(body) > CACHE_MAX_BYTES:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, suffix=".tmp", delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(body)
            stream.close()
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)
    size = 0
    entries = []
    for entry in path.parent.glob("*.json"):
        try:
            entries.append((entry.stat(), entry))
        except FileNotFoundError:
            continue
    for stat, entry in sorted(entries, key=lambda item: item[0].st_mtime, reverse=True):
        size += stat.st_size
        if size > CACHE_MAX_BYTES or time.time() - stat.st_mtime >= CACHE_MAX_AGE:
            entry.unlink(missing_ok=True)


def _preview(images, transform):
    overlay = images[..., :3].clone()
    _, height, width, _ = overlay.shape
    for i, box, hit, absent in zip(transform["source"], transform["boxes"], transform["detected"], transform["absent"]):
        if absent:
            continue
        x, y, w, h = map(round, box)
        x0, y0 = max(0, min(x, width - 1)), max(0, min(y, height - 1))
        x1, y1 = min(width, x0 + max(1, w)), min(height, y0 + max(1, h))
        channel = 1 if hit else 0
        for patch in (overlay[i, y0:min(y1,y0+2), x0:x1], overlay[i, max(y0,y1-2):y1, x0:x1],
                      overlay[i, y0:y1, x0:min(x1,x0+2)], overlay[i, y0:y1, max(x0,x1-2):x1]):
            patch.zero_()
            patch[..., channel] = 1
    return overlay


class BGXFaceTrack:
    @classmethod
    def INPUT_TYPES(cls):
        return {"required": {
            "video": ("STRING",), "identity_file": ("STRING",), "settings_json": ("STRING",),
            "analysis_json": ("STRING",), "selection_json": ("STRING",), "render_preview": ("BOOLEAN",),
        }, "optional": {"identity_reference": ("IMAGE",)}}

    RETURN_TYPES = ("IMAGE", "H3FACEXFORM", "IMAGE", "STRING", "INT", "INT", "INT", "IMAGE", "AUDIO", "FLOAT")
    FUNCTION = "run"
    CATEGORY = "H3/Face Refine"

    @classmethod
    def IS_CHANGED(cls, **_):
        # This node verifies the actual source bytes before reusing its own trajectory.
        return float("nan")

    def run(self, video, identity_file, settings_json, analysis_json, selection_json, render_preview, identity_reference=None):
        from comfy.model_management import throw_exception_if_processing_interrupted
        select_node, track_node, upstream = _upstream()
        settings = json.loads(settings_json)
        analysis, selection = json.loads(analysis_json), json.loads(selection_json)
        video = _input_file(video)
        if bool(identity_file) != (identity_reference is not None):
            raise ValueError("Identity reference metadata differs from image input")
        key = _cache_key(video, _input_file(identity_file) if identity_file else None,
                         settings, analysis, selection, upstream)
        path = _root() / f"{key}.json" if key else None
        cached = _cache_read(path) if path else None
        if cached is not None:
            images, audio, fps = upstream._load_video_components(str(video))
            options = settings["select"]
            images, audio, fps = upstream._trim_batch(images, audio, fps, options["skip_first_frames"],
                                                      options["frame_load_cap"], options["select_every_nth"])
            try:
                _validate_geometry(cached, images.shape)
                receipt = tracking_receipt(cached, fps, analysis, selection)
                if not receipt["ready"]["ok"]:
                    raise ValueError("Cached trajectory is not ready")
            except (ValueError, KeyError, TypeError, IndexError):
                print("[BGXFaceTrack] invalid cached trajectory; rebuilding")
            else:
                width, height = cached["canvas"]
                crops = images.new_empty((cached["frames"], height, width, 3))
                for index, (frame, box) in enumerate(zip(cached["source"], cached["boxes"])):
                    throw_exception_if_processing_interrupted()
                    crops[index:index+1] = upstream._affine_crop(images[frame:frame+1], box, width, height)
                print(f"[BGXFaceTrack] reuse {key} frames={cached['source_frames']}", flush=True)
                preview = _preview(images, cached) if render_preview else images
                return crops, cached, preview, "", width, height, cached["frames"], images, audio, fps

        print(f"[BGXFaceTrack] track {key or 'uncached'}", flush=True)
        images, audio, face_pick, _, _, _, fps = select_node().run(video=str(video), **settings["select"])
        crops, transform, _, report, width, height, count = track_node().run(
            images=images, face_pick=face_pick, identity_reference=identity_reference,
            **settings["track"])
        transform = json.loads(json.dumps(transform, allow_nan=False))
        _validate_geometry(transform, images.shape)
        receipt = tracking_receipt(transform, fps, analysis, selection)
        if not receipt["ready"]["ok"] and not render_preview:
            raise ValueError(receipt["ready"]["reasons"][0])
        if path and receipt["ready"]["ok"]:
            try:
                _cache_write(path, transform)
            except OSError as error:
                print(f"[BGXFaceTrack] trajectory cache unavailable: {error}")
        preview = _preview(images, transform) if render_preview else images
        return crops, transform, preview, report, width, height, count, images, audio, fps
