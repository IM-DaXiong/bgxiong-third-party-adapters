#!/usr/bin/env python3
"""Build the flat CDN directory from locked ZIPs and a verified signed index.

No source globbing, implicit files, or same-version replacement is permitted.
--check validates every installable entry without writing any file.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import sys
from types import SimpleNamespace
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "artifacts" / "cdn"
QUALIFICATION = ROOT / "artifacts" / "cdn-index" / "qualification"


class PackError(Exception):
    def __init__(self, message: str, exit_code: int = 1):
        super().__init__(message)
        self.exit_code = exit_code


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def relative_path(value: str) -> PurePosixPath:
    if not isinstance(value, str) or not value or "\\" in value or ":" in value:
        raise PackError(f"Invalid relative path: {value!r}")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in ("", ".", "..") for part in value.split("/")):
        raise PackError(f"Unsafe relative path: {value!r}")
    return path


def locked_packages() -> list[tuple[str, list[tuple[str, bytes]]]]:
    lock = json.loads((ROOT / "release/current.json").read_text(encoding="utf-8-sig"))
    if lock.get("schemaVersion") != 1 or not isinstance(lock.get("packages"), list):
        raise PackError("Invalid current release lock")
    packages = []
    seen = set()
    problems = []
    root = ROOT.resolve()
    eligibility_file = ROOT / "release/online-releases.json"
    eligibility = json.loads(eligibility_file.read_text(encoding="utf-8-sig")) if eligibility_file.is_file() else {}
    online_speech = {(item["pluginId"], item["version"]) for item in eligibility.get("packages", [])
                     if item.get("resourceVersion")}
    for package in lock["packages"]:
        plugin, version = package["pluginId"], package["version"]
        managed = package.get("kind") == "speechManaged" and (plugin, version) in online_speech
        if package.get("kind") != "installable" and not managed:
            continue
        if not re.fullmatch(r"[a-z0-9._-]+", plugin) or not re.fullmatch(r"[0-9A-Za-z.+-]+", version):
            raise PackError(f"Unsafe package identity: {plugin!r}@{version!r}")
        name = f"{plugin}-{version}.zip"
        if name in seen:
            raise PackError(f"Duplicate package: {name}")
        seen.add(name)
        files = []
        paths = set()
        for entry in package["files"]:
            archive_path = relative_path(entry["path"]).as_posix()
            if archive_path.casefold() in paths:
                raise PackError(f"Duplicate archive path: {name}: {archive_path}")
            paths.add(archive_path.casefold())
            source = root.joinpath(*relative_path(entry["source"]).parts).resolve()
            if not source.is_relative_to(root):
                raise PackError(f"Source escapes repository: {entry['source']}")
            if not source.is_file():
                problems.append(f"{name}: missing source {entry['source']}")
                continue
            data = source.read_bytes()
            expected = entry["sha256"]
            if not re.fullmatch(r"[0-9a-f]{64}", expected) or sha256(data) != expected:
                problems.append(f"{name}: SHA-256 mismatch: {entry['source']}")
                continue
            if PurePosixPath(archive_path).name != "WHERE-TO-INSTALL.txt":
                files.append((archive_path, data))
        required = {"provisioning.json", "prepare-resources.ps1", "adapter-pack/manifest.json"} if managed else {
            "adapter-install.json", "package/manifest.json"
        }
        missing = required - {path for path, _ in files}
        if missing:
            problems.append(f"{name}: missing locked installation files: {', '.join(sorted(missing))}")
        print(f"{name}: {len(files)} verified files", flush=True)
        packages.append((name, sorted(files)))
    print(f"installable packages: {len(packages)}", flush=True)
    if problems:
        raise PackError("\n".join(problems))
    if not packages:
        raise PackError("No installable packages in release lock; refusing empty CDN replacement")
    return sorted(packages)


def write_archive(target: Path, files: list[tuple[str, bytes]]) -> None:
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path, data in files:
            info = zipfile.ZipInfo(path, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = 0o644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


def published_bytes(name: str, files: list[tuple[str, bytes]]) -> bytes | None:
    # A01: historic published ZIP metadata is immutable. Git pins the bytes
    # independently of the mutable output directory and its SHA256SUMS.txt.
    pin_file = ROOT / "release/cdn-published.json"
    if not pin_file.exists():
        return None
    pins = json.loads(pin_file.read_text(encoding="utf-8-sig"))
    if set(pins) != {"schemaVersion", "files"} or pins["schemaVersion"] != 1:
        raise PackError("Invalid published CDN receipt")
    pin = pins["files"].get(name)
    if pin is None:
        return None
    path = DEST / name
    if not path.is_file():
        path = QUALIFICATION / name
    if not path.is_file():
        raise PackError(f"Restore the original published ZIP before building: {name}", 2)
    data = path.read_bytes()
    if len(data) != pin["bytes"] or sha256(data) != pin["sha256"]:
        raise PackError(f"Same version has different bytes (同版本不同字节): {name}; published receipt mismatch", 2)
    import io
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        names = archive.namelist()
        if len(names) != len(files) or set(names) != {p for p, _ in files}:
            raise PackError(f"Published ZIP inventory differs from release lock: {name}", 2)
        if any(archive.read(p) != content for p, content in files):
            raise PackError(f"Published ZIP contents differ from release lock: {name}", 2)
    return data


def assemble(packages, staging: Path) -> dict[str, tuple[int, str]]:
    inventory = {}
    for name, files in packages:
        target = staging / name
        published = published_bytes(name, files)
        if published is None and "adapter-pack/manifest.json" in dict(files):
            manifest = json.loads(dict(files)["adapter-pack/manifest.json"])
            plugin, version = manifest["pluginId"], manifest["version"]
            if name != f"{plugin}-{version}.zip":
                raise PackError(f"Managed speech ZIP identity differs: {name}")
            frozen = ROOT / "artifacts" / "current" / plugin / f"{version}.zip"
            with zipfile.ZipFile(frozen) as archive:
                names = archive.namelist()
                if len(names) != len(files) or set(names) != {path for path, _ in files}:
                    raise PackError(f"Managed speech ZIP inventory differs from the release lock: {name}")
                if any(archive.read(path) != content for path, content in files):
                    raise PackError(f"Managed speech ZIP contents differ from the release lock: {name}")
            published = frozen.read_bytes()
        if published is None:
            write_archive(target, files)
        else:
            target.write_bytes(published)
        data = target.read_bytes()
        digest = sha256(data)
        for directory in (DEST, QUALIFICATION):
            previous = directory / name
            if previous.exists() and sha256(previous.read_bytes()) != digest:
                raise PackError(f"Same version has different bytes (同版本不同字节): {name}", 2)
        inventory[name] = (len(data), digest)
    sums = "".join(f"{digest}  {name}\n" for name, (_, digest) in inventory.items())
    (staging / "SHA256SUMS.txt").write_bytes(sums.encode("utf-8"))
    return inventory


def publish(packages, *, qualification=False, check=False, local_only=False,
            required=(), source_locks=(), signed_index=None) -> None:
    destination = QUALIFICATION if qualification else DEST
    if not qualification:
        verifier = importlib.import_module("stage-cdn-upload-directory")
        if signed_index is None:
            latest_path = verifier.INDEX_DIR / "latest.json"
            if not latest_path.is_file():
                raise PackError("Missing production-signed latest.json; run sign-online-index-interactive.ps1", 2)
            signed_index = latest_path.read_bytes()
        publisher = verifier.load_publisher()
    artifacts = ROOT / "artifacts"
    artifacts.mkdir(exist_ok=True)
    staging = artifacts / f".cdn-staging-{os.getpid()}"
    backup = artifacts / f".cdn-backup-{os.getpid()}"
    lock_path = artifacts / ".cdn-pack.lock"
    try:
        lock_fd = os.open(lock_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    except FileExistsError as exc:
        raise PackError("Another CDN build is active, or a previous build requires inspection: .cdn-pack.lock") from exc
    old_moved = False
    installed = False
    try:
        if staging.exists() or backup.exists():
            raise PackError("Staging/backup already exists; inspect it rather than overwrite")
        staging.mkdir()
        zip_inventory = assemble(packages, staging)
        if not qualification:
            expected = publisher.validated_packages(SimpleNamespace(source_lock=source_locks), staging)
            report = verifier.inspect_index(publisher, signed_index, zip_inventory,
                                            verifier.parse_require(publisher, required),
                                            local_only=local_only, expected_packages=expected)
            if not report["ready"]:
                raise PackError(verifier.write_report(report))
            (staging / "latest.json").write_bytes(signed_index)
        if check:
            return
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.exists():
            destination.rename(backup)
            old_moved = True
        try:
            staging.rename(destination)
            installed = True
        except BaseException:
            if old_moved:
                backup.rename(destination)
                old_moved = False
            raise
        if old_moved:
            shutil.rmtree(backup)
            old_moved = False
        label = "Qualification only (unsigned)" if qualification else "Upload ready (latest.json verified)"
        print(f"{label}: {len(zip_inventory)} ZIPs -> {destination}", flush=True)
    finally:
        # Never delete the only surviving old directory if restoration failed.
        if staging.exists():
            shutil.rmtree(staging)
        os.close(lock_fd)
        lock_path.unlink()
        if old_moved and not installed:
            print(f"Original CDN backup retained: {backup}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="validate all locked packages; write nothing")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--signed", action="store_true", help="also verify the signed index in --check mode")
    mode.add_argument("--qualification", action="store_true", help="write unsigned ZIPs to the signing workspace for qualification")
    parser.add_argument("--local-only", action="store_true", help="verify the index against local publication facts only")
    args = parser.parse_args()
    try:
        packages = locked_packages()
        if args.check and not args.signed:
            for name, files in packages:
                published_bytes(name, files)
        else:
            publish(packages, qualification=args.qualification, check=args.check, local_only=args.local_only)
        return 0
    except Exception as exc:
        print(f"CDN pack failed: {exc}", file=sys.stderr)
        return exc.exit_code if isinstance(exc, PackError) else 1


if __name__ == "__main__":
    sys.exit(main())
