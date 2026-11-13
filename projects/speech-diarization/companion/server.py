"""Managed loopback companion. The SDK host owns processes and media authorization."""
import hashlib
import json
import os
from pathlib import Path
import re
import threading
import uuid
import sys
from concurrent.futures import ThreadPoolExecutor
from fastapi import FastAPI, File, Header, HTTPException, UploadFile, Depends
from pydantic import BaseModel, ConfigDict, Field
import soundfile as sf
sys.path.insert(0, str(Path(__file__).resolve().parent))
from engine import Engine, Cancelled, atomic_json

ROOT = Path(__file__).resolve().parent
STORE = Path(os.environ["BGX_TTS_STORE_ROOT"])
STORE.mkdir(parents=True, exist_ok=True)
INSTANCE = os.environ["BGX_MANAGED_INSTANCE_ID"]
RESOURCE = hashlib.sha256((ROOT / "resources.lock.json").read_bytes()).hexdigest()
engine = None
jobs = {}
lock = threading.RLock()
pool = ThreadPoolExecutor(max_workers=1)
app = FastAPI()


def authorize(x_bgx_instance: str = Header(default="")):
    if x_bgx_instance != INSTANCE:
        raise HTTPException(403, "INSTANCE_MISMATCH")


@app.on_event("startup")
def startup():
    global engine
    manifest = json.loads((ROOT / "resources.lock.json").read_text(encoding="utf-8"))
    for item in manifest["files"]:
        file = ROOT / item["path"]
        digest = hashlib.file_digest(file.open("rb"), "sha256").hexdigest()
        if digest != item["sha256"]:
            raise RuntimeError("RESOURCE_DIGEST_MISMATCH: " + item["path"])
    engine = Engine(ROOT / "models")


@app.get("/health")
def health():
    return {"status": "ok", "modelLoaded": engine is not None, "instanceId": INSTANCE}


@app.post("/v1/audio", dependencies=[Depends(authorize)])
async def upload(file: UploadFile = File(...)):
    folder = STORE / "audio"
    folder.mkdir(exist_ok=True)
    temporary = folder / (uuid.uuid4().hex + ".part")
    digest = hashlib.sha256()
    try:
        with temporary.open("wb") as stream:
            while block := await file.read(1024 * 1024):
                stream.write(block)
                digest.update(block)
            stream.flush()
            os.fsync(stream.fileno())
        with sf.SoundFile(temporary) as audio:
            if audio.samplerate != 16000 or audio.channels != 1 or len(audio) == 0:
                raise HTTPException(422, "NORMALIZED_AUDIO_REQUIRED")
        identity = digest.hexdigest()
        os.replace(temporary, folder / (identity + ".wav"))
        return {"audioId": identity}
    finally:
        temporary.unlink(missing_ok=True)
        await file.close()


class Job(BaseModel):
    model_config = ConfigDict(extra="forbid")
    requestKey: str = Field(min_length=1)
    audioId: str = Field(pattern="^[a-f0-9]{64}$")
    sourceFingerprint: str = Field(pattern="^[a-f0-9]{64}$")
    durationMs: int = Field(gt=0)
    speakerCount: int | None = Field(default=None, gt=0)


def folder_for(job_id):
    if not re.fullmatch("[a-f0-9]{64}", job_id):
        raise HTTPException(400, "INVALID_JOB_ID")
    folder = STORE / "jobs" / job_id
    if not (folder / "plan.json").is_file():
        raise HTTPException(404, "JOB_NOT_FOUND")
    return folder


def read_status(folder):
    return json.loads((folder / "status.json").read_text(encoding="utf-8"))


def state(status, fraction=None, message=None, result=None):
    category = {"failed": "vendorTaskFailed", "cancelled": "vendorTaskCancelled"}.get(status)
    return {"status": status, "outputs": [], "data": result, "usage": None,
            "progress": {"fraction": fraction, "message": message} if status in ("queued", "running") else None,
            "terminalFailure": {"category": category, "message": message, "vendorCode": None, "details": None} if category else None}


def ensure_running(job_id):
    folder = folder_for(job_id)
    frozen = json.loads((folder / "plan.json").read_text(encoding="utf-8"))
    if frozen["resourceFingerprint"] != RESOURCE:
        raise HTTPException(409, "FROZEN_DIARIZATION_RESOURCE_CHANGED")
    with lock:
        if job_id in jobs or read_status(folder)["status"] in ("succeeded", "failed", "cancelled"):
            return
        cancellation = threading.Event()
        jobs[job_id] = cancellation
        def run():
            try:
                plan = json.loads((folder / "plan.json").read_text(encoding="utf-8"))
                def update(fraction, message):
                    atomic_json(folder / "status.json", state("running", fraction, message))
                result = engine.run(STORE / "audio" / (plan["audioId"] + ".wav"), plan,
                                    folder / "checkpoints", cancellation.is_set, update)
                atomic_json(folder / "status.json", state("succeeded", 1, "complete", result))
            except Cancelled as error:
                atomic_json(folder / "status.json", state("cancelled", message=str(error)))
            except Exception as error:
                atomic_json(folder / "status.json", state("failed", message=str(error)))
            finally:
                with lock:
                    jobs.pop(job_id, None)
        pool.submit(run)


@app.post("/v1/jobs", dependencies=[Depends(authorize)])
def submit(spec: Job):
    plan = spec.model_dump()
    plan["resourceFingerprint"] = RESOURCE
    job_id = hashlib.sha256(json.dumps(plan, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    if not (STORE / "audio" / (spec.audioId + ".wav")).is_file():
        raise HTTPException(404, "AUDIO_NOT_FOUND")
    folder = STORE / "jobs" / job_id
    with lock:
        if not (folder / "plan.json").exists():
            folder.mkdir(parents=True, exist_ok=True)
            atomic_json(folder / "status.json", state("queued", 0, "queued"))
            atomic_json(folder / "plan.json", plan)
    ensure_running(job_id)
    return {"jobId": job_id}


@app.get("/v1/jobs/{job_id}", dependencies=[Depends(authorize)])
def query(job_id: str):
    folder = folder_for(job_id)
    ensure_running(job_id)
    return read_status(folder)


@app.post("/v1/jobs/{job_id}/cancel", dependencies=[Depends(authorize)])
def cancel(job_id: str):
    folder = folder_for(job_id)
    with lock:
        if job_id in jobs:
            jobs[job_id].set()
            return {"outcome": "remote_requested"}
        if read_status(folder)["status"] not in ("succeeded", "failed", "cancelled"):
            atomic_json(folder / "status.json", state("cancelled", message="USER_CANCELLED"))
        return {"outcome": "remote_confirmed"}


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=int(os.environ["BGX_TTS_SIDECAR_PORT"]), log_level="warning")
