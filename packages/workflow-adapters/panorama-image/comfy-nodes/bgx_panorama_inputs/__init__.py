"""Load owned uploads as native Comfy values. No rendering or model dependencies."""
from pathlib import Path
import folder_paths
from comfy_api.latest import IO, ComfyExtension, InputImpl, Types
import asyncio
import json
import sys
from aiohttp import web
from server import PromptServer

VIDEO_EXTENSIONS = {".mp4", ".webm", ".mov", ".mkv", ".avi", ".m4v"}
MESH_EXTENSIONS = {".glb", ".gltf", ".obj", ".fbx", ".stl", ".usdz", ".ply", ".splat", ".spz", ".ksplat"}
IMAGE_EXTENSIONS = {".png", ".jpg", ".jpeg", ".webp", ".bmp", ".tif", ".tiff", ".gif"}


def owned_file(name, extensions):
    if not isinstance(name, str) or not name or "\\" in name or ":" in name:
        raise ValueError("Input must be a relative uploaded filename")
    relative = Path(name)
    if relative.is_absolute() or any(part in (".", "..") for part in name.split("/")):
        raise ValueError("Input path escapes the uploaded files")
    root = Path(folder_paths.get_input_directory()).resolve()
    path = (root / relative).resolve(strict=True)
    if not path.is_file() or not path.is_relative_to(root) or path.suffix.lower() not in extensions:
        raise ValueError("Unsupported or unowned input file")
    return path


class BGXPanoramaFile3D(IO.ComfyNode):
    @classmethod
    def define_schema(cls):
        return IO.Schema(node_id="BGXPanoramaFile3D", display_name="BGX Panorama 3D Input",
                         category="Panorama Suite/BGX", inputs=[IO.String.Input("file", default="")],
                         outputs=[IO.File3DAny.Output()])

    @classmethod
    def execute(cls, file):
        path = owned_file(file, {".glb", ".gltf", ".obj", ".fbx", ".stl", ".usdz", ".ply", ".splat", ".spz", ".ksplat"})
        return IO.NodeOutput(Types.File3D(str(path)))


class BGXPanoramaVideo(IO.ComfyNode):
    @classmethod
    def define_schema(cls):
        return IO.Schema(node_id="BGXPanoramaVideo", display_name="BGX Panorama Video Input",
                         category="Panorama Suite/BGX", inputs=[IO.String.Input("file", default="")],
                         outputs=[IO.Video.Output()])

    @classmethod
    def execute(cls, file):
        path = owned_file(file, {".mp4", ".webm", ".mov", ".mkv", ".avi", ".m4v"})
        return IO.NodeOutput(InputImpl.VideoFromFile(str(path)))


class BGXPanoramaInputs(ComfyExtension):
    async def get_node_list(self):
        return [BGXPanoramaFile3D, BGXPanoramaVideo]


async def comfy_entrypoint():
    return BGXPanoramaInputs()


def upstream_module(suffix):
    # Comfy imports custom-node folders under installation-specific package names.
    # Use only already-loaded Panorama modules; never import a path supplied by a request.
    found = [module for name, module in list(sys.modules.items())
             if module is not None and name.endswith("comfyui_pano_suite." + suffix)]
    if len(found) != 1:
        raise ValueError("Exactly one supported Panorama Suite installation must be loaded")
    return found[0]


def input_preview(payload):
    if not isinstance(payload, dict) or set(payload) != {"file", "role", "stateJson", "stickerState"}:
        raise ValueError("Unknown input preview fields")
    role = payload["role"]
    if role not in {"background", "world", "sticker"}:
        raise ValueError("Invalid input role")
    path = owned_file(payload["file"], VIDEO_EXTENSIONS | MESH_EXTENSIONS | IMAGE_EXTENSIONS)
    if role == "world" and path.suffix.lower() not in MESH_EXTENSIONS:
        raise ValueError("World requires 3D media")
    if role == "background" and path.suffix.lower() in MESH_EXTENSIONS:
        raise ValueError("Background requires image or video")
    result = {}
    if path.suffix.lower() in VIDEO_EXTENSIONS:
        media = InputImpl.VideoFromFile(str(path))
        width, height = media.get_dimensions()
        result["video"] = {"kind": "VIDEO", "fps": float(media.get_frame_rate()),
                           "frames": media.get_frame_count(), "duration": media.get_duration(),
                           "width": width, "height": height}
    elif path.suffix.lower() in MESH_EXTENSIONS:
        media = upstream_module("file_3d")._read_scene_file(Types.File3D(str(path)), role)
    else:
        # Port placement does not read image pixels; dimensions and actual pixels stay
        # in the original file and browser, with no extra full-resolution tensor copy.
        media = None
    if role == "sticker":
        scene = upstream_module("core.scene_state")
        port = upstream_module("port_sticker")
        state = scene.load_scene_state(payload["stateJson"])
        parsed = scene.load_sticker_state(payload["stickerState"])
        if payload["stickerState"].strip() and parsed is None:
            raise ValueError("Unsupported sticker pose record")
        is_3d = path.suffix.lower() in MESH_EXTENSIONS
        handoff = None if is_3d or parsed is None else port._handoff_lanes(parsed, state["timeline"]["fps"])
        original = next((item for item in state["stickers"] if item["source"]["kind"] == "port"), None)
        if original is None:
            sticker = port._world_port_sticker(state, media) if is_3d else port._port_sticker(state, None)
        elif is_3d and original["placement"] == "sky":
            sticker = {**port._world_port_sticker(state, media, original["id"]),
                       **{key: original[key] for key in ("clip", "visible", "locked", "animation") if key in original}}
        else:
            sticker = original
        result["sticker"] = sticker
        result["nextStickerId"] = state["next_sticker_id"]
        if handoff is not None:
            result["handoff"] = {"placement": "sky", "lanes": handoff, "fps": state["timeline"]["fps"]}
    return result


@PromptServer.instance.routes.post("/bgx_panorama/input_preview")
async def preview_route(request):
    try:
        payload = await request.json()
        result = await asyncio.to_thread(input_preview, payload)
        return web.json_response(result)
    except (ValueError, TypeError, KeyError, OSError) as error:
        return web.json_response({"error": str(error)}, status=400)
