"""Detect a target on at most five already extracted presentation frames."""
from hashlib import file_digest
import math


def _box(coordinates, width, height):
    left, top, right, bottom = (float(value) for value in coordinates)
    if not all(math.isfinite(v) for v in (left, top, right, bottom)):
        return None
    x0 = min(max(left / width, 0.0), 1.0)
    y0 = min(max(top / height, 0.0), 1.0)
    x1 = min(max(right / width, x0), 1.0)
    y1 = min(max(bottom / height, y0), 1.0)
    if x1 <= x0 or y1 <= y0:
        return None
    return dict(x=x0, y=y0, width=x1-x0, height=y1-y0)


def detect_samples(samples, detector, confidence, resolve, image_read):
    if not isinstance(samples, list) or not 1 <= len(samples) <= 5:
        raise ValueError("Expected one to five ordered presentation frames")
    ids = [item["mediaId"] for item in samples]
    if len(set(ids)) != len(ids):
        raise ValueError("Duplicate presentation frame")
    if not math.isfinite(confidence) or not 0.05 <= confidence <= 0.95:
        raise ValueError("Invalid face detector confidence")
    first = None
    for sample in samples:
        path = resolve(sample["uploadName"])
        with open(path, "rb") as stream:
            actual = "sha256:" + file_digest(stream, "sha256").hexdigest()
        if sample["imageDigest"] != actual:
            raise ValueError("Presentation frame digest mismatch")
        frame = image_read(path)
        if frame is None or frame.shape[:2] != (sample["height"], sample["width"]):
            raise ValueError("Presentation frame dimensions mismatch")
        result = detector.predict(frame, conf=confidence, verbose=False)[0]
        boxes = [_box(b, sample["width"], sample["height"]) for b in result.boxes.xyxy.tolist()]
        boxes = sorted((b for b in boxes if b), key=lambda b: (b["x"]+b["width"]/2, b["y"]))
        candidates = [{"candidateId": f"face-{i}", "box": box} for i, box in enumerate(boxes[:128])]
        receipt = {
            "kind": "detectReceipt",
            "mediaId": sample["mediaId"],
            "imageDigest": actual,
            "candidates": candidates,
        }
        if first is None:
            first = receipt
        if candidates:
            return receipt
    return first


def run(body, resolve):
    import cv2
    from .face_track import _upstream

    _, _, upstream = _upstream()
    detector = upstream._load_detector("bbox/face_yolov8m.pt")
    return detect_samples(body["samples"], detector, float(body.get("confidence", .35)),
                          resolve, lambda path: cv2.imread(str(path)))
