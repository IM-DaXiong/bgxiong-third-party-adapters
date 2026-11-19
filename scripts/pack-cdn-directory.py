#!/usr/bin/env python3
"""Build the flat CDN directory exclusively from verified release/current.json.

No source globbing, implicit files, or same-version replacement is permitted.
--check validates every installable entry without writing any file.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "artifacts" / "cdn"


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
    for package in lock["packages"]:
        if package.get("kind") != "installable":
            continue
        plugin, version = package["pluginId"], package["version"]
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
        required = {"adapter-install.json", "package/manifest.json"}
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


def publish(packages: list[tuple[str, list[tuple[str, bytes]]]]) -> None:
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
        sums = []
        for name, files in packages:
            target = staging / name
            published = published_bytes(name, files)
            if published is None:
                write_archive(target, files)
            else:
                target.write_bytes(published)
            digest = sha256(target.read_bytes())
            previous = DEST / name
            if previous.exists() and sha256(previous.read_bytes()) != digest:
                # A released version is immutable, including ZIP metadata bytes.
                raise PackError(f"Same version has different bytes (同版本不同字节): {name}; original CDN directory retained", 2)
            sums.append(f"{digest}  {name}")
            print(name, digest, flush=True)
        (staging / "SHA256SUMS.txt").write_bytes(("\n".join(sums) + "\n").encode("utf-8"))
        if DEST.exists():
            DEST.rename(backup)
            old_moved = True
        try:
            staging.rename(DEST)
            installed = True
        except BaseException:
            if old_moved:
                backup.rename(DEST)
                old_moved = False
            raise
        if old_moved:
            shutil.rmtree(backup)
            old_moved = False
        print(f"packages {len(sums)} -> {DEST}", flush=True)
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
    args = parser.parse_args()
    try:
        packages = locked_packages()
        if args.check:
            for name, files in packages:
                published_bytes(name, files)
        else:
            publish(packages)
        return 0
    except (PackError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as exc:
        print(f"CDN pack failed: {exc}", file=sys.stderr)
        return exc.exit_code if isinstance(exc, PackError) else 1


if __name__ == "__main__":
    sys.exit(main())
