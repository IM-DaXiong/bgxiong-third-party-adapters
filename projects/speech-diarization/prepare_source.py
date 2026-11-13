"""Freeze audited upstream source/models and the current public SDK profile."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parent
SDK = ROOT.parents[2] / "bgxiong-adapter-sdk"
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--upstream", type=Path, required=True)
    parser.add_argument("--models-receipt", type=Path, required=True)
    args = parser.parse_args()
    target = ROOT / "companion/upstream"
    shutil.copytree(args.upstream / "speakerlab", target / "speakerlab", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    shutil.copy2(args.upstream / "LICENSE", target / "LICENSE")
    entry = target / "speakerlab/bin/infer_diarization.py"
    source = entry.read_text(encoding="utf-8")
    source = source.replace("from pyannote.audio import Inference, Model", "# Optional overlap model is not distributed.")
    source = source.replace("from modelscope.pipelines import pipeline", "# Local FSMN inference is supplied by the companion.")
    entry.write_text(source, encoding="utf-8", newline="\n")
    files = []
    for receipt in json.loads(args.models_receipt.read_text(encoding="utf-8")):
        folder = "campplus" if "campplus" in receipt["model"] else "fsmn"
        model = ROOT / "companion/models" / folder
        model.mkdir(parents=True, exist_ok=True)
        for item in Path(receipt["path"]).iterdir():
            if item.is_file() and item.name in ("model.pt", "campplus_cn_en_common.pt", "config.yaml", "configuration.json", "am.mvn", "README.md"):
                shutil.copy2(item, model / item.name)
                files.append({"path": (model / item.name).relative_to(ROOT / "companion").as_posix(), "sha256": hashlib.sha256(item.read_bytes()).hexdigest()})
    (ROOT / "companion/resources.lock.json").write_text(json.dumps({"schemaVersion": 1,
        "upstream": {"url": "https://github.com/modelscope/3D-Speaker", "commit": "065629c313eaf1a01c65c640c46d77e61e9607b4", "license": "Apache-2.0"},
        "models": [{"id": r["model"], "revision": r["revision"]} for r in json.loads(args.models_receipt.read_text(encoding="utf-8"))],
        "files": files}, indent=2), encoding="utf-8")
    schemas = []
    for name in ("speech-analysis-v1/diarize.input.schema.json", "speech-analysis-v1/diarize.output.schema.json", "speech-analysis-v1/resume.schema.json", "speech-v1/configuration.schema.json"):
        text = (SDK / "profiles" / name).read_text(encoding="utf-8").replace("\r\n", "\n")
        schemas.append({"id": json.loads(text)["$id"], "digest": hashlib.sha256(text.encode()).hexdigest(), "schemaJson": text})
    def ref(i):
        return {k: schemas[i][k] for k in ("id", "digest")}
    caps = {"schemaVersion": 2, "pluginId": "local.speech.diarization", "pluginVersion": "1.0.0", "capabilityContracts": [{
        "schemaVersion": 2, "localId": "diarization", "capabilityKey": "speech.diarize", "transportSlot": "generic",
        "modelIds": ["3dspeaker-campplus-fsmn-ahc"], "configurationSchema": ref(3),
        "operations": [{"id": "std/speech.diarize", "version": 1, "label": "Speaker diarization", "inputSchema": ref(0), "outputSchema": ref(1), "resumeSchema": ref(2),
                       "uiSchema": {"controls": []}, "mediaSlots": [{"role": "audio", "inputPointer": "/audio", "mediaKind": "audio", "minCount": 1, "maxCount": 1}],
                       "execution": {"mode": "asynchronous", "streaming": False, "cancellation": "best_effort", "resumable": True, "remoteIdempotency": True}, "outputSlots": []}], "schemas": schemas}]}
    (ROOT / "adapter/src/capabilities.json").write_text(json.dumps(caps, indent=2), encoding="utf-8", newline="\n")

if __name__ == "__main__":
    main()
