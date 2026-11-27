"""Normalize H3 crop geometry for the host-owned trajectory revision store."""
import math
import sys
from pathlib import Path


def normalize(transform):
    count = transform["source_frames"]
    rows = [{"state": "lost", "geometry": None} for _ in range(count)]
    segments = {}
    for start, end in transform["segments"]:
        segment = transform["source"][start]
        for index in range(start, end):
            if index in segments:
                raise ValueError("Overlapping tracker segments")
            segments[index] = segment
    seen = set()
    for i, source in enumerate(transform["source"]):
        if not 0 <= source < count or source in seen:
            raise ValueError("Invalid or duplicate source frame in trajectory")
        seen.add(source)
        if transform["absent"][i] or not transform["detected"][i]:
            continue
        rows[source] = {
            "state": "present",
            "geometry": {
                "crop": list(transform["boxes"][i]),
                "face": list(transform["face_rect"][i]),
                "weight": float(transform["weights"][i]),
                "segmentStart": segments[i],
            },
        }
    return {"canvas": list(transform["canvas"]), "sourceSize": list(transform["src_size"]),
            "cropFactor": transform["crop_factor"]}, rows


def _geometry(frame, size):
    crop = frame["crop"]
    face = frame["face"]
    if any(len(b) != 4 or not all(math.isfinite(v) for v in b) or min(b[2:]) <= 0
           for b in (crop, face)):
        raise ValueError("Invalid face tracking geometry")
    x, y, w, h = crop
    if x < 0 or y < 0 or x + w > size[0] + 1e-6 or y + h > size[1] + 1e-6:
        raise ValueError("Crop exceeds fixed source geometry")
    if not 0.0 <= frame["weight"] <= 1.0:
        raise ValueError("Invalid face tracking blend weight")
    return crop, face


def restore(layout, rows):
    """Build the H3 transform without redetecting a face, preserving source indices."""
    canvas, size = layout["canvas"], layout["sourceSize"]
    if len(canvas) != 2 or len(size) != 2 or not 0 < min(canvas) <= max(canvas) <= 768 or min(size) <= 0:
        raise ValueError("Invalid fixed trajectory layout")
    source, boxes, faces, weights, segments = [], [], [], [], []
    last_segment, last_source = None, -2
    for index, frame in enumerate(rows):
        state = frame["state"]
        if state in ("lost", "skip") and frame["geometry"] is None:
            continue
        if state != "present" or not isinstance(frame["geometry"], dict):
            raise ValueError("Unresolved face trajectory frame")
        geometry = frame["geometry"]
        crop, face = _geometry(geometry, size)
        segment = geometry["segmentStart"]
        if segment != last_segment or index != last_source + 1:
            segments.append([len(source), len(source)])
        last_segment, last_source = segment, index
        segments[-1][1] += 1
        source.append(index)
        boxes.append(crop)
        faces.append(face)
        weights.append(geometry["weight"])
    if not source:
        raise ValueError("没有可精修的人脸帧")
    return dict(canvas=canvas, src_size=size, source_frames=len(rows), frames=len(source),
                source=source, boxes=boxes, face_rect=faces, weights=weights, segments=segments,
                detected=[True]*len(source), absent=[False]*len(source), crop_factor=layout["cropFactor"])


def _cut_detector(threshold):
    # The pure Python wheel travels with the verified companion. Only this node
    # needs it, so installing the adapter does not rewrite the Comfy environment.
    wheel = Path(__file__).with_name("dependencies") / "scenedetect-0.7.3-py3-none-any.whl"
    if not wheel.is_file():
        raise ValueError("当前人脸精修包缺少自动切镜依赖，请重新安装对应版本")
    location = str(wheel)
    sys.path.insert(0, location)
    try:
        from scenedetect.common import FrameTimecode
        from scenedetect.detectors import AdaptiveDetector
    except ImportError as error:
        raise ValueError("ComfyUI 无法加载人脸精修的自动切镜依赖") from error
    finally:
        sys.path.remove(location)
    return AdaptiveDetector(adaptive_threshold=float(threshold)), FrameTimecode


def manual_pick(images, anchor_index, box, settings, upstream, interrupt, identity_reference=None):
    """Run the normal detector on this batch only. Seed the tracker with the user's exact box."""
    count, height, width, _ = images.shape
    if not 0 <= anchor_index < count:
        raise ValueError("Anchor outside presented frames")
    x, y, w, h = (box[key] for key in ("x", "y", "width", "height"))
    if not (0 <= x < x+w <= 1 and 0 <= y < y+h <= 1):
        raise ValueError("Anchor box exceeds presentation geometry")
    detector = upstream._load_detector(settings["detector"])
    cut_det, cut_tc = None, None
    if settings["cut_detection"] == "auto":
        cut_det, cut_tc = _cut_detector(settings["cut_threshold"])
    boxes, cuts = [], []
    for index, image in enumerate(images):
        interrupt()
        bgr = upstream._to_bgr_u8(image)
        if cut_det:
            cuts.extend(int(cut.get_frames()) for cut in cut_det.process_frame(cut_tc(index, fps=24.0), bgr))
        result = detector.predict(bgr, conf=settings["confidence"], verbose=False)[0]
        boxes.append(result.boxes.xyxy.tolist())
    manual = [x*width, y*height, (x+w)*width, (y+h)*height]
    segments = upstream._segments(count, cuts)
    bounds = {frame: (a, b) for a, b in segments for frame in range(a, b)}
    embedder = upstream._make_embedder(settings["identity_model"])
    anchor_faces = embedder.embed(images[anchor_index:anchor_index+1], [manual])
    reference = max(
        (candidate for candidate in anchor_faces if upstream._iou(candidate[0], manual) > .25),
        key=lambda candidate: upstream._iou(candidate[0], manual),
        default=None,
    )
    model_threshold = settings["identity_threshold"]
    threshold = model_threshold if model_threshold > 0 else embedder.default_threshold
    reference = reference[1] if reference is not None else (
        embedder.embed_reference(identity_reference, detector, settings["confidence"])
        if identity_reference is not None else None
    )

    selected = {anchor_index: manual}
    # Both passes start at the same named face. After losing it or crossing a
    # cut, continuity gives way to a positive identity match.
    for direction in (-1, 1):
        last, last_index = _center(manual), anchor_index
        stop = -1 if direction < 0 else count
        for index in range(anchor_index + direction, stop, direction):
            interrupt()
            if not boxes[index]:
                continue
            neighboring = abs(index-last_index) == 1 and bounds[index] == bounds[last_index]
            reachable = [candidate for candidate in boxes[index]
                         if upstream._within_reach(candidate, last, abs(index-last_index))]
            chosen = reachable[0] if neighboring and len(reachable) == 1 and len(boxes[index]) == 1 else None
            if chosen is None and reference is not None:
                embeddings = embedder.embed(images[index:index+1], boxes[index])
                ranked = [(emb, max(boxes[index], key=lambda b: upstream._iou(emb[0], b)))
                          for emb in embeddings]
                matches = [(emb, candidate) for emb, candidate in ranked
                           if upstream._iou(emb[0], candidate) > .3]
                match_index, score = embedder.best_match([emb for emb, _ in matches], reference)
                if match_index is not None and score >= threshold:
                    chosen = matches[match_index][1]
            if chosen is not None:
                selected[index] = chosen
                last, last_index = _center(chosen), index

    picks = []
    for start, end in segments:
        frame = next((i for i in range(start, end) if i in selected), None)
        if frame is not None:
            picks.append(dict(segment=[start,end], frame=frame, box=0))
        else:
            picks.append(dict(segment=[start,end], frame=-1, absent=True))
    return dict(frames=count, src_size=[width,height],
                boxes=[[selected[i]] if i in selected else [] for i in range(count)],
                confs=[[1.0] if i in selected else [] for i in range(count)],
                segments=segments, detector=settings["detector"], confidence=settings["confidence"], picks=picks)


def _center(box):
    left, top, right, bottom = box
    return ((left + right) / 2, (top + bottom) / 2, bottom - top)


def track_frames(images, anchor_index, box, settings, fixed_layout=None, identity_reference=None):
    """Run either initial tracking or tracking on the caller's exact repair slice."""
    from comfy.model_management import throw_exception_if_processing_interrupted
    from .face_track import _upstream

    _, track_node, upstream = _upstream()
    options = dict(settings["track"])
    if fixed_layout is not None:
        options.update(canvas_mode="manual",
                       canvas_width=fixed_layout["canvas"][0],
                       canvas_height=fixed_layout["canvas"][1])
    pick = manual_pick(images, anchor_index, box, options, upstream,
                       throw_exception_if_processing_interrupted, identity_reference)
    options.update(fallback_detector="none", identity_track=False, absent_shots="off")
    _, transform, _, _, _, _, _ = track_node().run(images=images, face_pick=pick, **options)
    layout, rows = normalize(transform)
    if fixed_layout is not None and layout != fixed_layout:
        raise ValueError("修正区间与当前轨迹的裁剪布局不一致")
    return layout, rows

def read_exact_frames(path, expected):
    """Decode a half-open slice by stream PTS. Keyframe preroll is never returned."""
    from collections import Counter
    from fractions import Fraction
    import av
    import cv2
    import torch

    if not expected:
        raise ValueError("No source frames requested")
    base = Fraction(expected[0]["timeBaseNumerator"], expected[0]["timeBaseDenominator"])
    earliest = expected[0]["pts"]
    if any(Fraction(row["timeBaseNumerator"], row["timeBaseDenominator"]) != base for row in expected):
        raise ValueError("Inconsistent source time base")
    images = []
    with av.open(str(path)) as container:
        stream = container.streams.video[0]
        if stream.time_base != base:
            raise ValueError("Uploaded source time base differs from frame index")
        stream.thread_type = "AUTO"
        if stream.start_time is not None and earliest > stream.start_time:
            container.seek(earliest, stream=stream, backward=True, any_frame=False)
        seen = Counter()
        completed = 0
        for frame in container.decode(stream):
            if frame.pts is None or frame.time_base != base:
                raise ValueError("Decoded frame timestamp missing or mismatched")
            ordinal = seen[frame.pts]
            seen[frame.pts] += 1
            key = frame.pts, ordinal
            row = expected[completed]
            required = row["pts"], row["ordinal"]
            if key < required:
                continue
            if key != required:
                raise ValueError("Decoded presentation frame differs from the frozen index")
            image = frame.to_ndarray(format="rgb24")
            sar = stream.sample_aspect_ratio or 1
            # Match FFmpeg's display rotation before converting to square pixels.
            rotate = frame.rotation % 360
            direction = {90: cv2.ROTATE_90_COUNTERCLOCKWISE, 180: cv2.ROTATE_180,
                         270: cv2.ROTATE_90_CLOCKWISE}.get(rotate)
            if direction is not None:
                image = cv2.rotate(image, direction)
                if rotate in (90, 270):
                    sar = 1 / sar
            width = max(1, int(image.shape[1] * sar + .5))
            if width != image.shape[1]:
                image = cv2.resize(image, (width, image.shape[0]), interpolation=cv2.INTER_LINEAR)
            images.append(image)
            completed += 1
            if completed == len(expected):
                break
    if len(images) != len(expected):
        raise ValueError("Source ended before the frozen repair range")
    return torch.stack([torch.from_numpy(image) for image in images]).to(torch.float32).div_(255)
