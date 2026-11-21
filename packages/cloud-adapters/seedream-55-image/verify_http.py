"""Verify the packaged Wasm through the released CLI and worker without network grants."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import uuid


FEATURES = ["stream-events-v1", "operation-input-preflight-v2"]
PLUGIN = "local.bgx.seedream-55-image"
MODEL = "doubao-seedream-5-5-261028"


def digest(value):
    return hashlib.sha256(value).hexdigest()


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def save(path, value):
    path.write_bytes(json.dumps(value, ensure_ascii=False, indent=2).encode() + b"\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("cli", "sdk", "worker", "package", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    paths = parser.parse_args()
    if not all(path.is_absolute() for path in vars(paths).values()):
        parser.error("Provide absolute paths")
    paths.output.mkdir(parents=True)
    workspace = paths.output / "workspace"

    configure = {
        "endpointBaseUrl": "https://ark.cn-beijing.volces.com",
        "credentialRef": "seedream-55-image-user-key",
    }
    host = {
        "bindingId": "seedream-55-offline-check",
        "bindingRevision": 1,
        "configurationRef": "fixture",
        "configuration": configure,
        "network": {"origins": [], "allowPrivate": False, "allowEmbeddedAuthorization": False,
                    "deniedHosts": [], "deniedNetworks": []},
        "credentials": [],
        "trust": {"publishers": [], "revokedPlugins": [], "revokedVersions": [], "revokedKeys": [],
                  "denylistedHosts": [], "isFresh": False, "supportedFeatures": FEATURES},
        "media": [],
        "mediaTtlMs": 60000,
        "transport": {"handshakeTimeoutMs": 10000, "writeTimeoutMs": 60000, "shutdownTimeoutMs": 10000,
                      "cancelGraceMs": 100},
        "probe": None,
    }
    host_file = paths.output / "host.json"
    save(host_file, host)
    common = [str(paths.cli), "--sdk", str(paths.sdk), "--worker", str(paths.worker)]

    def execute(command, label):
        run = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", timeout=90)
        save(paths.output / f"{label}.run.json", {
            "command": command, "exitCode": run.returncode, "stdout": run.stdout, "stderr": run.stderr,
        })
        return run

    def invoke(operation, payload, label):
        request = {
            "protocolVersion": 2, "operation": operation, "profileId": host["bindingId"],
            "capabilitySlot": "generic" if operation == "capabilities" else "image",
            "requestId": str(uuid.uuid4()), "budgetMs": 60000, "payloadJson": canonical(payload).decode(),
            "bindingId": host["bindingId"], "negotiatedVersion": 2, "enabledFeatures": ["stream-events-v1"],
        }
        source = paths.output / f"{label}.input.json"
        target = workspace / "receipts" / f"{label}.output.json"
        save(source, request)
        command = [common[0], "caps" if operation == "capabilities" else operation, *common[1:],
                   "--package", str(paths.package), "--workspace", str(workspace), "--config", str(host_file),
                   "--input", str(source), "--output", str(target)]
        if not workspace.exists():
            command.append("--create-workspace")
        run = execute(command, label)
        response = load(target) if target.exists() else {}
        return request, response, run

    check = execute([common[0], "check", *common[1:], "--project", str(paths.package)], "check")
    assert check.returncode == 0, check.stderr
    _, response, run = invoke("capabilities", {}, "capabilities")
    assert run.returncode == 0, run.stderr
    caps = json.loads(response["dataJson"])
    assert (caps["pluginId"], caps["pluginVersion"]) == (PLUGIN, "1.0.0")
    contracts = {entry["localId"]: entry for entry in caps["capabilityContracts"]}
    assert set(contracts) == {"image", "image-edit"}
    package = {"pluginId": PLUGIN, "version": "1.0.0",
               "digest": digest((paths.package / "manifest.sha256").read_bytes())}

    def preparation(local_id, fields, ready=True):
        contract = contracts[local_id]
        operation, = contract["operations"]
        assert contract["modelIds"] == [MODEL]
        assert operation["version"] == 1 and operation["inputProfile"] == "operation-input-v1"
        for schema in contract["schemas"]:
            assert "\r" not in schema["schemaJson"]
            assert digest(schema["schemaJson"].encode()) == schema["digest"]
        binding = host["bindingId"]
        identity = {
            "modelId": MODEL, "capabilityKey": "image.generate", "operationId": operation["id"],
            "operationVersion": 1, "inputSchemaDigest": operation["inputSchema"]["digest"],
        }
        selection = {
            "package": package, "bindingId": binding, "bindingRevision": 1, **identity,
            "capabilityId": digest(canonical([package, binding, local_id, MODEL, "image.generate", operation["id"], 1])),
            "configurationDigest": digest(canonical(configure)),
            "contractDigest": digest(canonical(contract)),
        }
        metadata = [{"mediaId": item["mediaId"], "kind": "image", "mime": "image/png", "sha256": "1" * 64}
                    for item in fields.get("referenceImages", [])]
        payload = {
            "schemaVersion": 2, "kind": "operation_input_preflight", "selection": selection,
            "configuration": configure, "environment": {"revisionDigest": "f" * 64, "ready": ready},
            "input": fields, "mediaMetadata": metadata,
        }
        payload["requestDigest"] = digest(canonical(payload))
        return payload, identity

    request_ids = []
    for name, local_id, fields, ready, expected in [
        ("generate", "image", {"prompt": "透明贴纸", "size": "2880x1440", "background": "transparent",
                               "outputFormat": "png", "watermark": True}, True, "ready"),
        ("defaults", "image", {"prompt": "测试"}, True, "ready"),
        ("invalid-jpeg", "image", {"prompt": "测试", "outputFormat": "jpeg", "background": "transparent"}, True, "rejected"),
        ("invalid-ratio", "image", {"prompt": "测试", "size": "4000x240"}, True, "rejected"),
        ("unbound", "image", {"prompt": "测试"}, False, "rejected"),
        ("ten-references", "image-edit", {"prompt": "测试", "referenceImages": [
            {"mediaId": f"reference-{i}"} for i in range(10)]}, True, "ready"),
    ]:
        payload, identity = preparation(local_id, fields, ready)
        request, response, run = invoke("probe", payload, name)
        assert run.returncode == 0, (name, run.stderr)
        result = json.loads(response["dataJson"])
        assert result["status"] == expected and result["requestDigest"] == payload["requestDigest"], result
        request_ids.append(request["requestId"])
        if name == "defaults":
            assert result["material"] == {"size": "2K", "responseFormat": "url", "outputFormat": "png",
                                          "background": "auto", "watermark": True, "referenceCount": 0}
            proof = load(workspace / "cli" / digest(request["requestId"].encode()) / "operation-preparation.json")
            submit = {"schemaVersion": 2, **identity, "configuration": configure, "input": {"prompt": "已更改"},
                      "media": [], "uploads": [], "environmentRevisionDigest": "f" * 64, "preparation": proof}
            _, stale, stale_run = invoke("submit", submit, "changed-input")
            error = stale.get("error") or json.loads(stale_run.stderr)
            assert "PREPARATION_STALE" in json.dumps(error), error

    assert contracts["image"]["operations"][0]["mediaSlots"] == []
    assert contracts["image-edit"]["operations"][0]["mediaSlots"][0]["maxCount"] == 10
    save(paths.output / "summary.json", {"passed": 8, "networkGrantCount": 0, "probeRequestIds": request_ids})


if __name__ == "__main__":
    main()
