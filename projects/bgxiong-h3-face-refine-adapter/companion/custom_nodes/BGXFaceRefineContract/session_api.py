import asyncio
from pathlib import Path
from aiohttp import web
import folder_paths
from server import PromptServer
from .sessions import save_session, load_session
from .face_track import _input_file


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

    @routes.get("/bgx_face_refine/health")
    async def health(request):
        return web.json_response({"version":"1.0.3","analysisSessions":True,"trackingReceipt":True,"sharedTracking":True})

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
