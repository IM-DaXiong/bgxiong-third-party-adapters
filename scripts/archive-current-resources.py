#!/usr/bin/env python3
"""Archive exact current-lock bytes for the frozen-resource owner, without build or upload.

The output is a temporary aggregate owner ZIP, not another installation tree.
Official release receipts are never written by this preparation command.
"""
from __future__ import annotations
import argparse
import datetime as dt
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import re
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def load_owner(host):
    spec = importlib.util.spec_from_file_location("adapter_frozen_resource_owner", host / "scripts/restore-third-party-release.py")
    owner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(owner)
    return owner


def prepare(root, host, output, receipt):
    root, host, output, receipt = (p.resolve() for p in (root, host, output, receipt))
    if receipt in (root / "release/current-artifact.json", root / "release/current-artifact.local.json"):
        raise ValueError("Preparation cannot replace an official or existing owner receipt")
    if output.is_relative_to(root / "artifacts/current"):
        raise ValueError("Aggregate archives do not belong in the unique installation tree")
    owner = load_owner(host)
    lock_path = root / "release/current.json"
    lock_bytes = lock_path.read_bytes()
    lock_digest = sha(lock_bytes.replace(b"\r\n", b"\n"))
    if output.name != "third-party-frozen-resources-" + lock_digest[:12] + ".zip":
        raise ValueError("Archive filename must use the canonical LF lock fingerprint: " + lock_digest[:12])
    lock = json.loads(lock_bytes)
    if lock.get("schemaVersion") != 1 or lock.get("projectName") != "bgxiong-third-party-adapters" or not lock.get("packages"):
        raise ValueError("Invalid/empty current lock")
    expected, identities = {}, set()
    for package in lock["packages"]:
        identity = (package["pluginId"], package["version"])
        if identity in identities:
            raise ValueError("Duplicate locked package")
        identities.add(identity)
        for item in package["files"]:
            name = item["source"]
            path = PurePosixPath(name)
            if path.parts[:4] != ("artifacts", "current", *identity):
                raise ValueError("Non-current or mismatched package source")
            target = owner.checked_target(root, name)
            if name.casefold() in expected or not re.fullmatch(r"[0-9a-f]{64}", item["sha256"]):
                raise ValueError("Duplicate source or invalid digest")
            data = target.read_bytes()
            if sha(data) != item["sha256"]:
                raise ValueError("Locked source digest mismatch: " + name)
            expected[name.casefold()] = (name, item["sha256"], data)
    if not expected:
        raise ValueError("No locked files")
    output.parent.mkdir(parents=True, exist_ok=True)
    reused = output.exists()
    if not reused:
        with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name, _, data in sorted(expected.values()):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.external_attr = 0o644 << 16
                archive.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    with zipfile.ZipFile(output) as archive:
        names = archive.namelist()
        if len(names) != len(expected) or set(names) != {v[0] for v in expected.values()}:
            raise ValueError("Existing/generated owner archive has a different file set; not overwritten")
        for name, digest, _ in expected.values():
            if sha(archive.read(name)) != digest:
                raise ValueError("Owner archive digest mismatch; not overwritten")
    if lock_path.read_bytes() != lock_bytes:
        raise ValueError("Current lock changed during preparation")
    result = {"schemaVersion": 1, "kind": "localFrozenAdapterArchiveCandidate", "complete": True,
              "uploaded": False, "doNotClaimDownloadable": True,
              "lockSha256": sha(lock_bytes.replace(b"\r\n", b"\n")),
              "archiveSha256": sha(output.read_bytes()), "archiveBytes": output.stat().st_size,
              "archiveFileName": output.name, "archivePath": str(output), "fileCount": len(expected),
              "packageCount": len(identities), "reusedExistingArchive": reused,
              "when": dt.datetime.now(dt.timezone(dt.timedelta(hours=8))).isoformat(timespec="seconds")}
    # Exercise the real restore owner in a disposable root, never the real user runtime.
    with tempfile.TemporaryDirectory(prefix="adapter-current-owner-restore-") as temp:
        isolated = Path(temp)
        (isolated / "release").mkdir()
        (isolated / "release/current.json").write_bytes(lock_bytes)
        (isolated / "release/current-artifact.json").write_text(json.dumps(result), encoding="utf-8")
        result["isolatedRestore"] = owner.restore(isolated, output)
        result["isolatedSecondRestore"] = owner.restore(isolated, output)
    receipt.parent.mkdir(parents=True, exist_ok=True)
    receipt.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adapter-root", type=Path, default=ROOT)
    parser.add_argument("--host-root", type=Path, default=ROOT.parent / "bgxiong-ai-story")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--receipt", required=True, type=Path)
    args = parser.parse_args()
    print(json.dumps(prepare(args.adapter_root, args.host_root, args.output, args.receipt), ensure_ascii=False))


if __name__ == "__main__":
    main()
