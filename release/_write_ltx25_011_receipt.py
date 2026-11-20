# ASCII-ONLY helper: write local 0.1.1 pack receipt.
import hashlib
import json
from pathlib import Path

root = Path(r"L:\00Dev\bgxiong-third-party-adapters\artifacts\current\local.bgx.ltx25\0.1.1")
pkg = root / "package"
manifest = json.loads((pkg / "manifest.json").read_text(encoding="utf-8"))
manifest_sha = hashlib.sha256((pkg / "manifest.json").read_bytes()).hexdigest()
receipt = {
    "schemaVersion": 1,
    "pluginId": "local.bgx.ltx25",
    "version": "0.1.1",
    "displayName": "LTX 2.5",
    "artifactSha256": manifest["artifactSha256"],
    "manifestSha256": manifest_sha,
    "installDirectory": "artifacts/current/local.bgx.ltx25/0.1.1",
    "checks": [
        {
            "command": "cargo +1.95.0 test --offline",
            "cwd": "packages/workflow-adapters/ltx25",
            "result": "19 passed",
        },
        {
            "command": "build.ps1 pack to artifacts/current/local.bgx.ltx25/0.1.1/package",
            "status": "PACKED",
        },
    ],
    "cdnPublished": False,
    "fix": "Fetch only required ComfyUI node classes instead of full /object_info (1 MiB buffer).",
    "installNote": "Select artifacts/current/local.bgx.ltx25/0.1.1/ containing adapter-install.json. Not published to CDN in this fix round.",
}
out = Path(r"L:\00Dev\bgxiong-third-party-adapters\release\ltx25-0.1.1.json")
out.write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(out)
print(json.dumps(receipt, indent=2))
