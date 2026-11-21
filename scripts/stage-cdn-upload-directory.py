#!/usr/bin/env python3
"""Stage or verify the complete production-signed CDN directory."""
from __future__ import annotations

import argparse
import hashlib
import importlib
import importlib.util
import json
import re
import sys
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]
CDN = ROOT / "artifacts" / "cdn"
INDEX_DIR = ROOT / "artifacts" / "cdn-index"
UPLOAD_PREFIX = "https://r.bgxiong.com/client/ai-story/bgxiong-third-party-adapters/"
HOST_MOD = ROOT.parent / "bgxiong-ai-story" / "src-tauri" / "src" / "comfyui" / "workflow" / "adapter_online_update" / "mod.rs"
PUBKEY_RE = re.compile(r'INDEX_PUBKEY_B64:\s*&str\s*=\s*"([^"]*)"')
KEY_ID_RE = re.compile(r'INDEX_KEY_ID:\s*&str\s*=\s*"([^"]*)"')


class StageError(Exception):
    def __init__(self, message: str, exit_code: int = 2):
        super().__init__(message)
        self.exit_code = exit_code


def load_publisher():
    spec = importlib.util.spec_from_file_location("publish_online_index", ROOT / "scripts" / "publish-online-index.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def host_public_trust() -> tuple[str, str]:
    text = HOST_MOD.read_text(encoding="utf-8")
    key_id = KEY_ID_RE.search(text)
    pubkey = PUBKEY_RE.search(text)
    if not key_id or not pubkey:
        raise StageError("Host public trust constants were not found (read-only public key only)")
    return key_id.group(1), pubkey.group(1)


def load_lock_zips() -> dict[str, tuple[int, str]]:
    lock = json.loads((ROOT / "release" / "current.json").read_text(encoding="utf-8-sig"))
    names = {}
    for package in lock.get("packages", []):
        if package.get("kind") != "installable":
            continue
        name = "%s-%s.zip" % (package["pluginId"], package["version"])
        path = CDN / name
        if not path.is_file():
            raise StageError("Missing CDN ZIP: %s" % name)
        data = path.read_bytes()
        names[name] = (len(data), sha256(data))
    return names


def verify_sums(zips: dict[str, tuple[int, str]]) -> None:
    sums = (CDN / "SHA256SUMS.txt").read_text(encoding="utf-8")
    parsed = {}
    for line in sums.splitlines():
        if not line:
            continue
        digest, name = line.split("  ", 1)
        if name in parsed:
            raise StageError("Duplicate ZIP in SHA256SUMS.txt: %s" % name)
        parsed[name] = digest
    expected = {name: digest for name, (_n, digest) in zips.items()}
    if parsed != expected:
        raise StageError("SHA256SUMS.txt does not match the locked CDN ZIP set")
    paths = {path.name for path in CDN.iterdir()}
    if paths - {"SHA256SUMS.txt", "latest.json"} != zips.keys():
        raise StageError("CDN directory contains files outside the locked ZIP set")


def inspect_index(mod, latest: bytes, zips: dict[str, tuple[int, str]], required: list[tuple[str, str]], local_only: bool = False, *, expected_packages=None) -> dict:
    key_id, pubkey = host_public_trust()
    if key_id != mod.KEY_ID:
        raise StageError("Publisher key ID differs from the application production key ID")
    payload, payload_sha = mod.verified_index(latest, pubkey)
    receipt = json.loads((ROOT / "release" / "online-index-receipt.json").read_text(encoding="utf-8-sig"))
    receipt_seq = receipt["sequence"]
    receipt_sha = receipt.get("payloadSha256")
    indexed = []
    problems = []
    for item in payload["packages"]:
        name = item.get("file")
        if not isinstance(name, str) or "/" in name or "\\" in name or name.startswith(".") or name not in zips:
            problems.append("Indexed file missing or unsafe: %s" % name)
            continue
        size, digest = zips[name]
        if item.get("bytes") != size or item.get("sha256") != digest:
            problems.append("Indexed ZIP bytes differ from local file: %s" % name)
        indexed.append({
            "pluginId": item.get("pluginId"),
            "version": item.get("version"),
            "newInstall": item.get("newInstall"),
            "file": name,
            "bytes": size,
            "sha256": digest,
        })
    if problems:
        raise StageError("\n".join(problems))
    if expected_packages is not None and payload["packages"] != expected_packages:
        raise StageError("latest.json does not match current online qualification; sign the current batch")
    missing_required = []
    for plugin, version in required:
        hit = next((p for p in indexed if p["pluginId"] == plugin and p["version"] == version), None)
        if not hit:
            missing_required.append("%s@%s is not in the signed index" % (plugin, version))
        elif hit["newInstall"] is not True:
            missing_required.append("%s@%s is in the signed index but newInstall is not true" % (plugin, version))
    public_sha = None
    public_seq = None
    public_error = None
    if not local_only:
        try:
            public_bytes = mod.public_index_bytes()
            public_payload, public_payload_sha = mod.verified_index(public_bytes, pubkey)
            public_sha = sha256(public_bytes)
            public_seq = public_payload["sequence"]
        except Exception as exc:
            public_error = str(exc)
    seq = payload["sequence"]
    rollback = []
    if public_seq is not None and seq < public_seq:
        rollback.append("signed sequence %s would roll back public sequence %s" % (seq, public_seq))
    if seq < receipt_seq:
        rollback.append("signed sequence %s would roll back receipt sequence %s" % (seq, receipt_seq))
    if seq == receipt_seq and receipt_sha and payload_sha != receipt_sha:
        rollback.append("same receipt sequence with different payload bytes")
    if public_seq is not None and (public_seq < receipt_seq or
                                   (public_seq == receipt_seq and public_payload_sha != receipt_sha)):
        rollback.append("public index is older than the publication receipt or differs at the same sequence")
    if public_seq is not None and seq == public_seq and public_sha and sha256(latest) != public_sha:
        rollback.append("same public sequence with different latest.json bytes")
    ready = (not rollback) and (not missing_required) and (public_error is None)
    return {
        "ready": ready,
        "keyId": key_id,
        "sequence": seq,
        "payloadSha256": payload_sha,
        "latestSha256": sha256(latest),
        "latestBytes": len(latest),
        "receiptSequence": receipt_seq,
        "nextSequenceAfterReceipt": receipt_seq + 1,
        "publicSequence": public_seq,
        "publicLatestSha256": public_sha,
        "publicError": public_error,
        "rollback": rollback,
        "indexed": indexed,
        "zipCount": len(zips),
        "required": ["%s@%s" % item for item in required],
        "missingRequired": missing_required,
        "uploadPrefix": UPLOAD_PREFIX,
        "destination": str(CDN),
    }


def parse_require(mod, values: list[str]) -> list[tuple[str, str]]:
    required = []
    for value in values:
        plugin, _, version = value.partition("@")
        if not re.fullmatch(r"[a-z0-9._-]+", plugin) or not mod.stable_version(version):
            raise StageError("Invalid --require-new-install identity: %s" % value)
        required.append((plugin, version))
    return required


def write_report(report: dict) -> str:
    lines = [
        "ready=%s" % report["ready"],
        "signedSequence=%s" % report["sequence"],
        "receiptSequence=%s" % report["receiptSequence"],
        "publicSequence=%s" % report["publicSequence"],
        "indexedPackages=%s" % len(report["indexed"]),
        "localZips=%s" % report["zipCount"],
        "required=%s" % (",".join(report["required"]) or "-"),
    ]
    if report["missingRequired"]:
        lines.append("missingRequired:")
        lines.extend("  - %s" % item for item in report["missingRequired"])
    if report["rollback"]:
        lines.append("rollback:")
        lines.extend("  - %s" % item for item in report["rollback"])
    if report["publicError"]:
        lines.append("publicError=%s" % report["publicError"])
    lines.append("indexed:")
    for item in report["indexed"]:
        lines.append("  - %s@%s newInstall=%s %s" % (item["pluginId"], item["version"], item["newInstall"], item["file"]))
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="report readiness; write nothing")
    parser.add_argument("--require-new-install", action="append", default=[], metavar="pluginId@version",
                        help="refuse to write unless this exact package is signed with newInstall=true")
    parser.add_argument("--local-only", action="store_true", help="verify against local publication facts only")
    parser.add_argument("--source-lock", type=Path, action="append", default=[])
    args = parser.parse_args()
    try:
        if not args.check:
            packer = importlib.import_module("pack-cdn-directory")
            packer.publish(packer.locked_packages(), local_only=args.local_only,
                           required=args.require_new_install, source_locks=args.source_lock)
            return 0
        mod = load_publisher()
        required = parse_require(mod, args.require_new_install)
        latest_path = CDN / "latest.json"
        if not latest_path.is_file():
            raise StageError("Upload directory is incomplete: missing production-signed latest.json")
        latest = latest_path.read_bytes()
        zips = load_lock_zips()
        verify_sums(zips)
        expected = mod.validated_packages(SimpleNamespace(source_lock=args.source_lock), CDN)
        report = inspect_index(mod, latest, zips, required, local_only=args.local_only,
                               expected_packages=expected)
        sys.stdout.write(write_report(report))
        return 0 if report["ready"] else 2
    except StageError as exc:
        print("CDN upload staging failed: %s" % exc, file=sys.stderr)
        return exc.exit_code
    except Exception as exc:
        print("CDN upload staging failed: %s" % exc, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
