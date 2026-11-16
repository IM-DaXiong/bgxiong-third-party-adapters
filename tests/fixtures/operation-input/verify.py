"""Build and exercise both test-only author packages with one frozen SDK."""

from argparse import ArgumentParser
from copy import deepcopy
from hashlib import sha256
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tomllib
import zlib


FIXTURE = Path(__file__).resolve().parent
PLUGIN = "local.bgx.operation-input-fixture"
BINDING = "operation-input-fixture"
FEATURE = "operation-input-preflight-v2"
MIX_POINTER = "/layout~1options/mix~0ratio"


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def load(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def hash_bytes(content):
    return sha256(content).hexdigest()


def digest(value):
    return hash_bytes(encode(value).encode())


def hash_file(path):
    return hash_bytes(path.read_bytes())


def fixture_png():
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    row = b"\x00" + b"\x54\x7e\x69" * 128
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", 128, 128, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(row * 128))
        + chunk(b"IEND", b"")
    )


def host_config():
    return {
        "bindingId": BINDING, "bindingRevision": 1, "configurationRef": BINDING,
        "configuration": {}, "credentials": [], "mediaTtlMs": 600000, "probe": None,
        "network": {"origins": [], "allowPrivate": False, "allowEmbeddedAuthorization": False,
                    "deniedHosts": [], "deniedNetworks": []},
        "trust": {"publishers": [], "revokedPlugins": [], "revokedVersions": [], "revokedKeys": [],
                  "denylistedHosts": [], "isFresh": False, "supportedFeatures": [FEATURE]},
        "transport": {"handshakeTimeoutMs": 10000, "writeTimeoutMs": 20000,
                      "shutdownTimeoutMs": 10000, "cancelGraceMs": 100},
    }


def sparse_index_path(name):
    if len(name) <= 2:
        return f"{len(name)}/{name}"
    if len(name) == 3:
        return f"3/{name[0]}/{name}"
    return f"{name[:2]}/{name[2:4]}/{name}"


def seed_locked_registry(source, author):
    archive_dirs = sorted((source / "cache").glob("index.crates.io-*"))
    if len(archive_dirs) != 1:
        raise ValueError("Expected one prepared crates.io archive directory")
    archives, = archive_dirs
    index = source / "index" / archives.name
    target = author / ".bgx-sdk" / "cargo-home" / "registry"
    official_index = target / "index" / archives.name
    target_archives = target / "cache" / archives.name
    official_index.mkdir(parents=True)
    target_archives.mkdir(parents=True)
    shutil.copyfile(index / "config.json", official_index / "config.json")
    lock = tomllib.loads((author / "Cargo.lock").read_text(encoding="utf-8"))
    for package in lock["package"]:
        if package.get("source") != "registry+https://github.com/rust-lang/crates.io-index":
            continue
        name, version = package["name"], package["version"]
        archive = archives / f"{name}-{version}.crate"
        if hash_file(archive) != package["checksum"]:
            raise ValueError(f"Cached archive hash differs from Cargo.lock: {name} {version}")
        sparse = Path(".cache") / sparse_index_path(name)
        (official_index / sparse).parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(index / sparse, official_index / sparse)
        shutil.copyfile(archive, target_archives / archive.name)


class Evidence:
    def __init__(self, sdk, output, registry):
        self.source = sdk / "source"
        self.runtime = sdk / "runtime" / "adapter-sdk-v2"
        self.cli = sdk / "runtime" / "bgx-adapter-v2.exe"
        self.worker = self.runtime / "bgx-adapter-v2-worker.exe"
        self.output = output
        self.registry = registry
        self.commands = []

    def call(self, label, *args):
        argv = [str(self.cli), *map(str, args)]
        process = subprocess.run(argv, capture_output=True, encoding="utf-8", timeout=240)
        self.commands.append({"label": label, "argv": argv, "exitCode": process.returncode})
        save(self.output / "commands.json", self.commands)
        (self.output / f"{label}.stdout.log").write_text(process.stdout, encoding="utf-8")
        (self.output / f"{label}.stderr.log").write_text(process.stderr, encoding="utf-8")
        if process.returncode != 0:
            detail = process.stderr or process.stdout
            raise RuntimeError(f"{label} failed with exit {process.returncode}: {detail[-1200:]}")

    def build(self, version, profile):
        author = self.output / f"author-{version}"
        self.call(f"{version}-init", "init", "--sdk", self.source, "--dir", author)
        shutil.copyfile(FIXTURE / "src" / "lib.rs", author / "src" / "lib.rs")
        (author / "schemas").mkdir(exist_ok=True)
        shutil.copyfile(FIXTURE / "schemas" / f"{profile}.json", author / "schemas" / "input.json")
        (author / "assets").mkdir()
        (author / "assets" / "fixture.png").write_bytes(fixture_png())
        for name in ("Cargo.toml", "Cargo.lock"):
            path = author / name
            old = 'name = "bgx-adapter-template"\nversion = "1.0.0"'
            assert old in path.read_text(encoding="utf-8"), path
            new = f'name = "bgx-adapter-template"\nversion = "{version}"'
            path.write_text(path.read_text(encoding="utf-8").replace(old, new, 1), encoding="utf-8")

        manifest_path = author / "manifest.json"
        manifest = load(manifest_path)
        manifest.update(pluginId=PLUGIN, displayName="参数输入测试包", version=version, requiredFeatures=[FEATURE])
        manifest["requestedCapabilities"]["mediaSlots"] = ["image"]
        manifest["limits"]["wallMs"] = 15000
        save(manifest_path, manifest)
        seed_locked_registry(self.registry, author)
        self.call(f"{version}-build", "build", "--sdk", self.source, "--project", author, "--offline")

        bundle = self.output / f"candidate-{version}"
        bundle.mkdir()
        package = bundle / "package"
        self.call(f"{version}-pack", "pack", "--sdk", self.source, "--project", author, "--out", package)
        self.call(f"{version}-check", "check", "--sdk", self.runtime, "--worker", self.worker,
                  "--project", package)
        install_config = host_config()
        install_config["budgetMs"] = {"capabilities": 60000, "probe": 60000, "submit": 90000,
                                      "query": 60000, "cancel": 60000}
        save(bundle / "adapter-install.json", {
            "schemaVersion": 1, "packageDirectory": "package",
            "manifestSha256": hash_file(package / "manifest.json"),
            "hostConfig": install_config, "authentication": {"source": "none"}, "managedMediaProbe": False,
        })
        shutil.copyfile(FIXTURE.parents[2] / "LICENSE", bundle / "LICENSE")
        return package


class Run:
    def __init__(self, evidence, version, package):
        self.evidence = evidence
        self.version = version
        self.package = package
        self.root = evidence.output / f"run-{version}"
        self.root.mkdir()
        self.workspace = self.root / "workspace"
        self.config_path = self.root / "config.json"
        save(self.config_path, {**host_config(), "media": []})
        catalog = self.invoke("capabilities", "discover", {})
        assert catalog["pluginVersion"] == version
        self.contract, = catalog["capabilityContracts"]
        self.operation, = self.contract["operations"]
        self.package_identity = {"pluginId": PLUGIN, "version": version,
                                 "digest": hash_file(package / "manifest.sha256")}
        self.operation_identity = {
            "capabilityKey": self.contract["capabilityKey"], "modelId": self.contract["modelIds"][0],
            "operationId": self.operation["id"], "operationVersion": self.operation["version"],
            "inputSchemaDigest": self.operation["inputSchema"]["digest"],
        }
        self.selection = {
            **self.operation_identity, "package": self.package_identity,
            "bindingId": BINDING, "bindingRevision": 1, "configurationDigest": digest({}),
            "contractDigest": digest(self.contract),
            "capabilityId": digest([self.package_identity, BINDING, self.contract["localId"],
                                    self.operation_identity["modelId"], self.contract["capabilityKey"],
                                    self.operation["id"], self.operation["version"]]),
        }
        self.environment = {"ready": True, "revisionDigest": "e" * 64}
        self.input_schema = json.loads(next(
            s["schemaJson"] for s in self.contract["schemas"] if s["id"] == "input"
        ))

    def invoke(self, operation, request_id, payload):
        path = self.root / f"{request_id}.request.json"
        result = self.workspace / "receipts" / f"{request_id}.json"
        save(path, {"protocolVersion": 2, "operation": operation, "profileId": BINDING,
                    "bindingId": BINDING, "capabilitySlot": "generic" if operation == "capabilities" else "image",
                    "requestId": request_id, "budgetMs": 90000, "negotiatedVersion": 2,
                    "enabledFeatures": [], "payloadJson": encode(payload)})
        args = [operation, "--sdk", self.evidence.runtime, "--worker", self.evidence.worker,
                "--package", self.package, "--workspace", self.workspace, "--config", self.config_path,
                "--input", path, "--output", result]
        if not self.workspace.exists():
            args.append("--create-workspace")
        self.evidence.call(f"{self.version}-{request_id}", *args)
        return json.loads(load(result)["dataJson"])

    def prepare(self, request_id, values):
        payload = {"schemaVersion": 2, "kind": "operation_input_preflight", "selection": self.selection,
                   "configuration": {}, "input": values, "environment": self.environment, "mediaMetadata": []}
        payload["requestDigest"] = digest(payload)
        outcome = self.invoke("probe", f"{request_id}-prepare", payload)
        assert outcome["requestDigest"] == payload["requestDigest"]
        return outcome

    def complete(self, case, values):
        outcome = self.prepare(case, values)
        assert outcome["status"] == "ready", outcome
        accepted = outcome["material"]
        assert accepted == {"received": values, "operationVersion": self.operation["version"]}
        preparation_path = self.workspace / "cli" / hash_bytes(f"{case}-prepare".encode()) / "operation-preparation.json"
        preparation = load(preparation_path)
        assert preparation["material"] == accepted
        result = self.invoke("submit", case, {"schemaVersion": 2, **self.operation_identity,
            "configuration": {}, "input": values, "media": [], "uploads": [],
            "environmentRevisionDigest": self.environment["revisionDigest"], "preparation": preparation})
        assert result["kind"] == "completed" and result["data"] == accepted, result
        output, = result["outputs"]
        media = output["media"]
        original = self.workspace / "objects" / media["fileName"]
        assert media["kind"] == "image" and media["mime"] == "image/png", media
        assert original.read_bytes() == fixture_png()
        return {
            "caseId": case, "package": self.package_identity, "operation": self.operation_identity,
            "inputDigest": digest(values), "receivedDigest": digest(result["data"]["received"]),
            "preparationSha256": hash_file(preparation_path), "outputSha256": hash_file(original),
            "received": result["data"]["received"], "status": "completed", "vendorTaskId": result["vendorTaskId"],
        }

    def reject(self, case, values, pointer):
        outcome = self.prepare(case, values)
        assert outcome["status"] == "rejected", outcome
        diagnostic, = outcome["diagnostics"]
        assert diagnostic["fieldPointer"] == pointer, diagnostic
        assert diagnostic["code"] == f"{PLUGIN}/invalid_input", diagnostic
        request_file = self.root / f"{case}.request.json"
        assert not request_file.exists(), request_file
        return {"caseId": case, "package": self.package_identity, "operation": self.operation_identity,
                "inputDigest": digest(values), "preparationRequestDigest": outcome["requestDigest"],
                "status": "rejected", "diagnostic": diagnostic, "submitCalls": 0}


def main():
    parser = ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", required=True, type=Path, help="Frozen SDK build root with source, runtime and receipt")
    parser.add_argument("--cargo-registry", required=True, type=Path,
                        help="Prepared crates.io registry containing index and pinned crate archives")
    parser.add_argument("--output", required=True, type=Path, help="New local evidence directory")
    args = parser.parse_args()
    sdk = args.sdk.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    evidence = Evidence(sdk, output, args.cargo_registry.resolve(strict=True))
    receipt = sdk / "build-receipt.json"
    def runtime_protocol_hashes():
        return {path.relative_to(evidence.runtime).as_posix(): hash_file(path)
                for path in sorted((evidence.runtime / "protocol").glob("*.json"))}

    fixed = {"sourceCommit": load(receipt)["sourceCommit"], "sourceDigest": load(receipt)["sourceDigest"],
             "sdkBuildReceiptSha256": hash_file(receipt), "cliSha256": hash_file(evidence.cli),
             "workerSha256": hash_file(evidence.worker), "runtimeProtocolSha256": runtime_protocol_hashes()}
    baseline = Run(evidence, "1.0.0", evidence.build("1.0.0", "baseline"))
    extended = Run(evidence, "1.1.0", evidence.build("1.1.0", "extended"))
    baseline_tones = baseline.input_schema["properties"]["inkTone"]["enum"]
    extended_tones = extended.input_schema["properties"]["inkTone"]["enum"]
    assert extended_tones == [*baseline_tones, "sage"]
    assert "grainLevel" not in baseline.input_schema["properties"]
    controls = {c["fieldPointer"]: (c["label"], c["widget"]) for c in extended.operation["uiSchema"]["controls"]}
    assert controls["/grainLevel"] == ("颗粒强度", "number")
    assert controls[MIX_POINTER] == ("混合比例", "number")
    assert controls["/passes"] == ("处理顺序", "repeat")
    shared = {"prompt": "合成参数测试", "inkTone": "clay", "glossEnabled": True}
    additions = {"prompt": "验证新参数保持原值", "inkTone": "sage", "glossEnabled": True,
                 "grainLevel": 0.375, "layout/options": {"mix~ratio": 0.625},
                 "passes": [{"mode": "base", "gain/value": 0}, {"mode": "overlay", "gain/value": 0.25}]}
    excess_mix = deepcopy(additions)
    excess_mix["layout/options"]["mix~ratio"] = 0.875
    low_gain = deepcopy(additions)
    low_gain["passes"][1]["gain/value"] = 0.0625
    rows = [
        baseline.complete("V06-baseline-accepts", shared),
        extended.reject("V06-extended-rejects", shared, "/glossEnabled"),
        extended.complete("V01-V02-extended-values", additions),
        extended.reject("V02-escaped-object-field", excess_mix, MIX_POINTER),
        extended.reject("V02-escaped-list-field", low_gain, "/passes/1/gain~1value"),
    ]
    assert hash_file(evidence.cli) == fixed["cliSha256"]
    assert hash_file(evidence.worker) == fixed["workerSha256"]
    assert hash_file(receipt) == fixed["sdkBuildReceiptSha256"]
    assert runtime_protocol_hashes() == fixed["runtimeProtocolSha256"]
    packages = [{"version": run.version, "bundle": str(run.package.parent),
                 "manifestSha256": hash_file(run.package / "manifest.json"),
                 "wasmSha256": hash_file(run.package / "adapter.wasm"),
                 "envelopeSha256": hash_file(run.package.parent / "adapter-install.json")}
                for run in (baseline, extended)]
    report = {"scope": "independent SDK CLI and actual WASM worker", "sdk": fixed,
              "packages": packages, "results": rows, "externalRequests": 0,
              "untested": ["fixed desktop EXE", "UI field rendering and saved draft readback", "application upgrade"]}
    save(output / "summary.json", report)
    print(encode({"result": "PASS", "cases": len(rows), "evidence": str(output / "summary.json")}))


if __name__ == "__main__":
    main()
