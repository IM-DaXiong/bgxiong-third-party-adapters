"""Build the complete BGX desktop install envelope from a reviewed WASM."""
import argparse
import hashlib
import json
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PLUGIN = "local.bgx.cloud-asr"
VERSION = "0.1.1"
ORIGIN = "https://www.bgxiong.com"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--wasm", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    if not args.wasm.is_file():
        parser.error("build the WASM first")
    output = args.out
    output.mkdir(parents=True, exist_ok=False)
    package = output / "package"
    package.mkdir()
    wasm = package / "adapter.wasm"
    shutil.copyfile(args.wasm, wasm)
    manifest = package / "manifest.json"
    write_json(manifest, {
        "schemaVersion": 2, "pluginId": PLUGIN, "version": VERSION, "displayName": "比格熊语音识别",
        "entry": "adapter.wasm", "language": "rust", "protocolRange": {"min": 2, "max": 2},
        "artifactSha256": sha(wasm), "publisherId": None, "publisherKeyId": None,
        "requiredFeatures": [], "optionalFeatures": [],
        "requestedCapabilities": {
            "endpointPolicy": ["userConfigurable", "allowPrivateNetwork"], "networkHosts": [],
            "allowPrivateNetwork": True, "auth": ["bearer"], "mediaSlots": ["audio"],
            "stateBytes": 0, "embeddedHeaders": [],
        },
        "limits": {"memoryBytes": 64 * 1024 * 1024, "fuel": 1000000000, "wallMs": 30000, "outputBytes": 1048576},
    })
    (package / "manifest.sha256").write_text(
        f"{sha(wasm)} adapter.wasm\n{sha(manifest)} manifest.json\n", encoding="utf-8")
    write_json(output / "adapter-install.json", {
        "schemaVersion": 1, "packageDirectory": "package", "manifestSha256": sha(manifest),
        "hostConfig": {
            "bindingId": "bgxiong-cloud-asr", "bindingRevision": 1, "configurationRef": "cloud-asr-v1",
            "configuration": {"endpointBaseUrl": ORIGIN, "credentialRef": "bgx-asr-task-grant"},
            "network": {"origins": [ORIGIN], "allowPrivate": False,
                        "allowEmbeddedAuthorization": False, "deniedHosts": [], "deniedNetworks": []},
            "credentials": [],
            "trust": {"publishers": [], "revokedPlugins": [], "revokedVersions": [], "revokedKeys": [],
                      "denylistedHosts": [], "isFresh": False, "supportedFeatures": []},
            "mediaTtlMs": 180000,
            "transport": {"handshakeTimeoutMs": 10000, "writeTimeoutMs": 15000,
                          "shutdownTimeoutMs": 10000, "cancelGraceMs": 100},
            "budgetMs": {"capabilities": 15000, "probe": 15000, "submit": 25000,
                         "query": 25000, "cancel": 15000},
            "probe": None,
        },
        "authentication": {"source": "none"}, "managedMediaProbe": False,
    })
    shutil.copyfile(ROOT / "README.md", output / "README.md")
    shutil.copyfile(ROOT.parents[2] / "LICENSE", output / "LICENSE")
    print(f"{PLUGIN} {VERSION} {sha(wasm)}")


if __name__ == "__main__":
    main()
