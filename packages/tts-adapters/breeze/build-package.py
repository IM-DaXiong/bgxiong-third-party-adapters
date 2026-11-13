"""Build a complete immutable managed-speech bundle from the sibling SDK and source."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import zipfile

SOURCE = Path(__file__).resolve().parent
REPO = SOURCE.parents[2]


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--sdk", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    destination = args.out.resolve()
    archive = Path(str(destination) + ".zip")
    if destination.exists() or archive.exists():
        parser.error("Output and ZIP must be new; published package bytes are immutable")
    sdk_source = REPO.parent / "bgxiong-adapter-sdk"
    subprocess.run(["cargo", "build", "--offline", "--locked", "--release", "--target", "wasm32-wasip2",
                    "--manifest-path", str(SOURCE / "adapter/Cargo.toml")], check=True)
    wasm = SOURCE / "adapter/target/wasm32-wasip2/release/bgx_breeze_speech.wasm"
    manifest = read(SOURCE / "adapter/manifest.json")
    if manifest["version"] != "1.0.1" or "submission-resolution-v1" not in manifest["requiredFeatures"]:
        raise RuntimeError("This producer requires the durable 1.0.1 package contract")
    manifest["artifactSha256"] = sha(wasm)
    write(SOURCE / "adapter/manifest.json", manifest)
    shutil.copyfile(wasm, SOURCE / "adapter/adapter.wasm")
    # Existing inventory is the explicit source allowlist, not a recursive copy
    # that could pick up model weights, temporary references or a developer venv.
    inventory_path = SOURCE / "companion/files.json"
    paths = {row["path"] for row in read(inventory_path)["files"]} | {"durable_jobs.py"}
    files = []
    for relative in sorted(paths):
        source = SOURCE / "companion" / relative
        if not source.resolve().is_relative_to((SOURCE / "companion").resolve()):
            raise RuntimeError("Unsafe companion inventory path")
        files.append({"path": relative, "sha256": sha(source), "byteLength": str(source.stat().st_size)})
    write(inventory_path, {"schemaVersion": 1, "files": files})
    bundle = read(SOURCE / "tts-bundle.json")
    bundle["adapter"] = {"path": "adapter/adapter.wasm", "sha256": sha(wasm)}
    bundle["runtimes"][0]["code"]["sha256"] = sha(inventory_path)
    write(SOURCE / "tts-bundle.json", bundle)
    provisioning = read(SOURCE / "provisioning.json")
    provisioning["pluginVersion"] = manifest["version"]
    provisioning["adapterSha256"] = sha(wasm)
    write(SOURCE / "provisioning.json", provisioning)
    pack = destination / "adapter-pack"
    pack.mkdir(parents=True)
    for name in ("manifest.json", "adapter.wasm"):
        shutil.copyfile(SOURCE / "adapter" / name, pack / name)
    inventory = "".join(f"{sha(pack / name)} {name}\n" for name in ("adapter.wasm", "manifest.json"))
    (pack / "manifest.sha256").write_text(inventory, encoding="ascii", newline="\n")
    (SOURCE / "adapter/manifest.sha256").write_text(inventory, encoding="ascii", newline="\n")
    bundle["adapter"]["path"] = "adapter.wasm"
    write(pack / "tts-bundle.json", bundle)
    for row in files:
        target = pack / "companion" / row["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(SOURCE / "companion" / row["path"], target)
    shutil.copyfile(inventory_path, pack / "companion/files.json")
    shutil.copyfile(SOURCE / "provisioning.json", destination / "provisioning.json")
    shutil.copyfile(SOURCE / "installation/prepare-resources.ps1", destination / "prepare-resources.ps1")
    shutil.copyfile(REPO / "LICENSE", destination / "LICENSE")
    shutil.copyfile(SOURCE / "README.md", destination / "README.md")
    checked = subprocess.run([str(args.cli.resolve()), "check", "--sdk", str(args.sdk.resolve()),
                    "--worker", str(args.worker.resolve()), "--project", str(pack)], capture_output=True, text=True, encoding="utf-8")
    if checked.returncode:
        raise RuntimeError("SDK package check failed: " + checked.stdout + checked.stderr)
    write(destination / "package-check.json", json.loads(checked.stdout))
    sdk_head = subprocess.check_output(["git", "-C", str(sdk_source), "rev-parse", "HEAD"], text=True).strip()
    source_head = subprocess.check_output(["git", "-C", str(REPO), "rev-parse", "HEAD"], text=True).strip()
    write(destination / "build-receipt.json", {
        "schemaVersion": 1, "pluginId": manifest["pluginId"], "version": manifest["version"],
        "adapterSha256": sha(wasm), "sdkSourceBaseCommit": sdk_head, "adapterSourceBaseCommit": source_head,
        "sdkSourceDirty": bool(subprocess.check_output(["git", "-C", str(sdk_source), "status", "--porcelain"], text=True).strip()),
        "sdkCliSha256": sha(args.cli), "sdkWorkerSha256": sha(args.worker),
        "runtimeVersion": provisioning["version"], "modelInferenceAcceptance": "pending-separate-host-test",
        "sdkBuildReceiptSha256": sha(args.sdk.parent / "build-receipt.json") if (args.sdk.parent / "build-receipt.json").is_file() else None,
    })
    (destination / "SHA256SUMS.txt").write_text("".join(
        f"{sha(file)}  {file.relative_to(destination).as_posix()}\n"
        for file in sorted(destination.rglob("*")) if file.is_file()), encoding="utf-8", newline="\n")
    with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED) as zipped:
        for file in sorted(destination.rglob("*")):
            if file.is_file():
                zipped.write(file, file.relative_to(destination).as_posix())
    print(json.dumps({"directory": str(destination), "archive": str(archive), "sha256": sha(archive),
                      "bytes": archive.stat().st_size, "version": manifest["version"]}))


if __name__ == "__main__":
    main()
