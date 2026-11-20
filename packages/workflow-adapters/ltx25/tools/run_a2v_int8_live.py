#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Live A2V INT8 run against local ComfyUI, mirroring adapter build_graph + /upload/image + /prompt."""
from __future__ import annotations

import json
import math
import struct
import sys
import time
import uuid
import wave
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path

SHANGHAI = timezone(timedelta(hours=8), name="Asia/Shanghai")
BASE = "http://127.0.0.1:8188"
ROOT = Path(r"L:\00Dev\bgxiong-third-party-adapters")
GRAPH_PATH = ROOT / r"packages/workflow-adapters/ltx25/workflows/a2v.api.json"
OUT_DIR = ROOT / r"release/evidence"
WAV_PATH = OUT_DIR / "20261120-ltx25-a2v-int8-test.wav"
RESULT_PATH = OUT_DIR / "20261120-ltx25-a2v-int8-comfy-live.json"
LOG_PATH = OUT_DIR / "20261120-ltx25-a2v-int8-comfy-live.log"
LOCK_PATH = OUT_DIR / "20261120-ltx25-a2v-int8-comfy-live.lock"
COMFY_OUTPUT = Path(r"L:\AI\ComfyUI-Installs\ComfyUI (1)\ComfyUI\output")

DEFAULT_PROMPT = (
    "The person shown in the first frame remains the same person and talks while looking toward the camera. "
    "Mouth movement, blinking, and small natural shifts in expression and head position follow the timing of the supplied speech. "
    "Face, hair, clothing, and background stay consistent with the first frame. "
    "This is a single continuous take with a static camera, holding a medium close-up of the head and shoulders, with no pan, zoom, or cut. "
    "Lighting stays unchanged from the opening shot."
)
DURATION = 2
FPS = 24
WIDTH = 768
HEIGHT = 512
SEED = 0
AUDIO_START = 0.0
SAVE_NODE = "4852"
POLL_SECONDS = 8
TIMEOUT_SECONDS = 45 * 60

def now_iso() -> str:
    return datetime.now(SHANGHAI).isoformat(timespec="seconds")

def log(msg: str) -> None:
    line = f"[{now_iso()}] {msg}"
    print(line, flush=True)
    with LOG_PATH.open("a", encoding="utf-8") as fh:
        fh.write(line + "\n")

def http_json(method: str, path: str, body=None, timeout=60):
    data = None
    headers = {}
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(BASE + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read()
            return resp.status, json.loads(raw.decode("utf-8")) if raw else {}
    except urllib.error.HTTPError as exc:
        raw = exc.read()
        try:
            parsed = json.loads(raw.decode("utf-8"))
        except Exception:
            parsed = {"raw": raw.decode("utf-8", errors="replace")}
        return exc.code, parsed

def write_wav() -> None:
    rate = 16000
    nframes = rate * DURATION
    with wave.open(str(WAV_PATH), "w") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        frames = bytearray()
        for i in range(nframes):
            sample = int(0.15 * 32767.0 * math.sin(2.0 * math.pi * 440.0 * i / rate))
            frames += struct.pack("<h", sample)
        wav.writeframes(frames)
    log(f"wrote wav {WAV_PATH} bytes={WAV_PATH.stat().st_size} duration={DURATION}s 440Hz")

def upload_audio(path: Path) -> str:
    boundary = "----bgxltx25" + uuid.uuid4().hex
    filename = path.name
    file_bytes = path.read_bytes()
    chunks = []
    def add_field(name: str, value: str) -> None:
        chunks.append(f"--{boundary}\r\n".encode("utf-8"))
        chunks.append(f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode("utf-8"))
        chunks.append(value.encode("utf-8") + b"\r\n")
    chunks.append(f"--{boundary}\r\n".encode("utf-8"))
    chunks.append(
        f'Content-Disposition: form-data; name="image"; filename="{filename}"\r\n'.encode("utf-8")
    )
    chunks.append(b"Content-Type: audio/wav\r\n\r\n")
    chunks.append(file_bytes)
    chunks.append(b"\r\n")
    add_field("type", "input")
    add_field("overwrite", "true")
    chunks.append(f"--{boundary}--\r\n".encode("utf-8"))
    body = b"".join(chunks)
    req = urllib.request.Request(
        BASE + "/upload/image",
        data=body,
        method="POST",
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        payload = json.loads(resp.read().decode("utf-8"))
    name = payload["name"]
    sub = payload.get("subfolder") or ""
    ref = f"{sub}/{name}" if sub else name
    log(f"uploaded audio ref={ref} payload={payload}")
    return ref

def write_number(graph: dict, node: str, field: str, value: float) -> None:
    cls = graph[node]["class_type"]
    if cls == "PrimitiveFloat":
        graph[node]["inputs"][field] = float(value)
    else:
        graph[node]["inputs"][field] = int(value)

def build_graph(audio_ref: str) -> dict:
    graph = json.loads(GRAPH_PATH.read_text(encoding="utf-8"))
    if graph["5014:5556"]["inputs"]["switch"] is not False:
        raise SystemExit("frozen graph drift: enhance switch is not false")
    graph["5508"]["inputs"]["value"] = DEFAULT_PROMPT
    write_number(graph, "5512", "value", float(DURATION))
    write_number(graph, "5511", "value", float(FPS))
    write_number(graph, "5514:3059", "width", WIDTH // 2)
    write_number(graph, "5514:3059", "height", HEIGHT // 2)
    write_number(graph, "5516:4832", "noise_seed", SEED)
    write_number(graph, "5517:4967", "noise_seed", SEED)
    write_number(graph, "5601", "value", AUDIO_START)
    graph["5600"]["inputs"]["audio"] = audio_ref
    graph["2004"] = {
        "class_type": "EmptyImage",
        "inputs": {"width": 512, "height": 512, "batch_size": 1, "color": 0},
        "_meta": {"title": "No first frame"},
    }
    graph["5014:5506"]["inputs"]["value"] = False
    if graph["5014:5556"]["inputs"]["switch"] is not False:
        raise SystemExit("frozen graph drift after patch")
    expected = {
        "5575:5569": "ltx-2.5-22b-distilled-transformer-comfy-int8-convrot.safetensors",
        "5575:5572": "gemma4_e2b_it_int8_convrot.safetensors",
        "5575:5574": "gemma4-12b-with-proj-ltx-2.5-comfy-int8-convrot.safetensors",
    }
    fields = {"5575:5569": "unet_name", "5575:5572": "clip_name", "5575:5574": "clip_name"}
    for nid, want in expected.items():
        actual = graph[nid]["inputs"][fields[nid]]
        if actual != want:
            raise SystemExit(f"graph still has unexpected weight {nid}: {actual}")
    log(
        f"patched graph width/2={WIDTH//2} height/2={HEIGHT//2} duration={DURATION} fps={FPS} "
        f"seed={SEED} audio={audio_ref} emptyFirstFrame=True"
    )
    return graph

def find_output_files(outputs: dict) -> list[dict]:
    files = []
    node = outputs.get(SAVE_NODE) or {}
    for key in ("videos", "gifs", "images", "audio", "files"):
        for item in node.get(key) or []:
            filename = item.get("filename") or ""
            subfolder = item.get("subfolder") or ""
            typ = item.get("type") or "output"
            rel = str(Path(subfolder) / filename) if subfolder else filename
            disk = COMFY_OUTPUT / subfolder / filename if subfolder else COMFY_OUTPUT / filename
            size = disk.stat().st_size if disk.is_file() else None
            files.append({
                "kind": key,
                "filename": filename,
                "subfolder": subfolder,
                "type": typ,
                "diskPath": str(disk),
                "bytes": size,
                "exists": disk.is_file(),
            })
    return files

def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    try:
        lock = open(LOCK_PATH, "x", encoding="utf-8")
    except FileExistsError:
        log("lock exists; refusing to submit a second live job")
        return 2
    try:
        lock.write(now_iso() + "\n")
        lock.flush()
        status, queue = http_json("GET", "/queue")
        running = queue.get("queue_running") or []
        pending = queue.get("queue_pending") or []
        log(f"queue running={len(running)} pending={len(pending)}")
        if running or pending:
            log("ComfyUI already has jobs; aborting to avoid colliding")
            json.dumps
            RESULT_PATH.write_text(json.dumps({"ok": False, "reason": "queue not empty", "queue": queue}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            return 3
        status, stats = http_json("GET", "/system_stats")
        dev = (stats.get("devices") or [{}])[0]
        log(f"vram_free_before_free={dev.get('vram_free')} torch_free={dev.get('torch_vram_free')}")
        _, freed = http_json("POST", "/free", {"unload_models": True, "free_memory": True})
        time.sleep(2)
        _, stats2 = http_json("GET", "/system_stats")
        dev2 = (stats2.get("devices") or [{}])[0]
        log(f"vram_free_after_free={dev2.get('vram_free')} torch_free={dev2.get('torch_vram_free')} free_resp={freed}")
        write_wav()
        audio_ref = upload_audio(WAV_PATH)
        graph = build_graph(audio_ref)
        client_id = "bgx-ltx25-a2v-int8-" + uuid.uuid4().hex[:12]
        started = time.time()
        status, submitted = http_json("POST", "/prompt", {"prompt": graph, "client_id": client_id}, timeout=120)
        log(f"submit status={status} body={json.dumps(submitted, ensure_ascii=False)[:2000]}")
        if status >= 400 or "prompt_id" not in submitted:
            RESULT_PATH.write_text(json.dumps({"ok": False, "submitStatus": status, "submit": submitted}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            return 4
        prompt_id = submitted["prompt_id"]
        log(f"prompt_id={prompt_id}")
        terminal = None
        while time.time() - started < TIMEOUT_SECONDS:
            _, hist = http_json("GET", f"/history/{prompt_id}")
            entry = hist.get(prompt_id) if isinstance(hist, dict) else None
            if entry:
                status_obj = entry.get("status") or {}
                completed = status_obj.get("completed")
                status_str = status_obj.get("status_str")
                messages = status_obj.get("messages") or []
                log(f"history completed={completed} status_str={status_str} messages={len(messages)}")
                if completed or status_str in {"success", "error"} or entry.get("outputs"):
                    terminal = entry
                    break
            _, q = http_json("GET", "/queue")
            run_ids = []
            for item in (q.get("queue_running") or []) + (q.get("queue_pending") or []):
                # typical item: [number, prompt_id, prompt, extra]
                if isinstance(item, list) and len(item) > 1:
                    run_ids.append(str(item[1]))
            log(f"still waiting; in_queue={prompt_id in run_ids} elapsed={int(time.time()-started)}s")
            time.sleep(POLL_SECONDS)
        elapsed = time.time() - started
        if terminal is None:
            log("TIMEOUT waiting for history")
            RESULT_PATH.write_text(json.dumps({"ok": False, "prompt_id": prompt_id, "elapsedSec": elapsed, "reason": "timeout"}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            return 5
        status_obj = terminal.get("status") or {}
        outputs = terminal.get("outputs") or {}
        files = find_output_files(outputs)
        ok = (status_obj.get("status_str") == "success" or status_obj.get("completed") is True) and any(
            (f["exists"] and (f["bytes"] or 0) > 0) for f in files
        )
        # also surface node errors
        result = {
            "ok": ok,
            "prompt_id": prompt_id,
            "client_id": client_id,
            "elapsedSec": round(elapsed, 2),
            "recordedAt": now_iso(),
            "params": {
                "durationSeconds": DURATION,
                "fps": FPS,
                "width": WIDTH,
                "height": HEIGHT,
                "latentWidth": WIDTH // 2,
                "latentHeight": HEIGHT // 2,
                "seed": SEED,
                "audioStartSeconds": AUDIO_START,
                "audioFile": str(WAV_PATH),
                "audioRef": audio_ref,
                "emptyFirstFrame": True,
                "prompt": DEFAULT_PROMPT,
            },
            "status": status_obj,
            "outputs": outputs.get(SAVE_NODE),
            "files": files,
        }
        RESULT_PATH.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        log(f"DONE ok={ok} elapsed={elapsed:.1f}s files={files}")
        if not ok:
            # dump a compact error view
            log("status messages: " + json.dumps(status_obj, ensure_ascii=False)[:4000])
            return 6
        return 0
    finally:
        lock.close()

if __name__ == "__main__":
    sys.exit(main())
