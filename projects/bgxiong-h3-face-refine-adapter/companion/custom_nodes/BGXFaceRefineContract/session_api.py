import asyncio
import json
import uuid
from pathlib import Path
from aiohttp import web
import folder_paths
from server import PromptServer
from .sessions import save_session, load_session
from .face_track import _input_file
from . import seed, trajectory


def has_audio(name):
    import av

    if not isinstance(name, str) or not name:
        raise ValueError("Missing uploaded video name")
    with av.open(str(_input_file(name))) as media:
        if not media.streams.video:
            raise ValueError("Uploaded source has no video stream")
        return bool(media.streams.audio)


def register():
    root = Path(folder_paths.get_temp_directory()) / "bgx-face-refine" / "analysis-v1"
    routes = PromptServer.instance.routes
    trajectory_root = root.parent / "trajectory-download-v1"
    detector_lock = asyncio.Lock()

    @routes.get("/bgx_face_refine/health")
    async def health(request):
        return web.json_response({"version":"1.0.4","analysisSessions":True,"trackingReceipt":True,"sharedTracking":True,"seedFrames":True,"trajectoryArtifacts":True})

    @routes.post("/bgx_face_refine/detect-samples")
    async def detect_samples(request):
        try:
            body = await request.json()
            if PromptServer.instance.prompt_queue.get_current_queue()[0]:
                return web.json_response({"error": "ComfyUI 正在执行任务，请稍后识别人脸"}, status=409)
            async with detector_lock:
                result = await asyncio.to_thread(seed.run, body, _input_file)
            return web.json_response(result)
        except (ValueError, TypeError, KeyError, OSError) as error:
            return web.json_response({"error": str(error)}, status=400)

    @routes.post("/bgx_face_refine/trajectory")
    async def make_trajectory(request):
        try:
            body = await request.json()
            async with detector_lock:
                if PromptServer.instance.prompt_queue.get_current_queue()[0]:
                    return web.json_response({"error": "ComfyUI 正在执行任务，请稍后跟踪"}, status=409)
                output_id = await asyncio.to_thread(compute_trajectory, body, trajectory_root)
            return web.json_response({"artifactId": output_id})
        except (ValueError, TypeError, KeyError, OSError) as error:
            return web.json_response({"error": str(error)}, status=400)

    @routes.get("/bgx_face_refine/trajectory/{artifact_id}")
    async def get_trajectory(request):
        try:
            name = uuid.UUID(hex=request.match_info["artifact_id"]).hex
        except ValueError:
            return web.json_response({"error": "Invalid trajectory receipt"}, status=400)
        path = trajectory_root / f"{name}.json"
        if not path.is_file():
            return web.json_response({"error": "Trajectory artifact unavailable"}, status=404)
        return web.FileResponse(path, headers={
            "Content-Type": "application/octet-stream",
            "Content-Disposition": f'attachment; filename="{name}.bin"',
        })

    @routes.post("/bgx_face_refine/source-audio")
    async def source_audio(request):
        import av

        try:
            body = await request.json()
            audio = await asyncio.to_thread(has_audio, body["video"])
            return web.json_response({"hasAudio": audio})
        except (OSError, av.FFmpegError, ValueError, KeyError, TypeError):
            return web.json_response({"error": "Could not verify uploaded video audio streams"}, status=400)

    @routes.post("/bgx_face_refine/analysis-session")
    async def store(request):
        try:
            value = await request.json()
            await asyncio.to_thread(save_session, root, value)
            return web.json_response({"saved":True})
        except (ValueError, KeyError, TypeError) as error:
            return web.json_response({"error":str(error)},status=400)

    @routes.get("/bgx_face_refine/analysis-session/{analysis_id}")
    async def read(request):
        try:
            return web.json_response(await asyncio.to_thread(load_session, root, request.match_info["analysis_id"]))
        except FileNotFoundError:
            return web.json_response({"error":"Analysis session unavailable; analyze again"},status=404)
        except (ValueError, KeyError, TypeError) as error:
            return web.json_response({"error":str(error)},status=400)

def compute_trajectory(body, output_root):
    from PIL import Image
    import numpy as np
    import torch

    video = _input_file(body["video"])
    frames = body["frames"]
    anchor_index = body["anchorIndex"]
    if not isinstance(frames, list) or not 0 <= anchor_index < len(frames):
        raise ValueError("Invalid trajectory request")
    images = trajectory.read_exact_frames(video, frames)
    reference = None
    if body.get("identityImage"):
        with Image.open(_input_file(body["identityImage"])) as image:
            reference = torch.from_numpy(np.array(image.convert("RGB"))).to(torch.float32).div_(255).unsqueeze(0)
    layout, rows = trajectory.track_frames(images, anchor_index, body["anchorBox"],
                                          body["settings"], body.get("layout"), reference)
    value = {"layout": layout, "frames": rows}
    artifact_id = uuid.uuid4().hex
    output_root.mkdir(parents=True, exist_ok=True)
    destination = output_root / (artifact_id + ".json")
    temporary = destination.with_suffix(".tmp")
    try:
        with temporary.open("x", encoding="utf8") as output:
            json.dump(value, output, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)
    return artifact_id
