#!/usr/bin/env python3
"""Pack a ComfyUI custom-node source ZIP from an explicit upstream git commit.

The archive uses a fixed path order, 1980-01-01 timestamps, Unix attributes,
and DEFLATE level 9 so the same input yields identical bytes. A sidecar
manifest records the archive URL, SHA-256, size, requirements and wheel lock
for later envelope / CDN work (A07, A09, A21).

This script never guesses a commit from a hand-installed ComfyUI directory,
never invents an upstream digest, and never uses the network.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path, PurePosixPath
from typing import Iterable, Mapping
from urllib.parse import urlparse

PRODUCTION_ARCHIVE_BASE = "https://r.bgxiong.com/client/ai-story/comfy-custom-nodes"
ALLOWED_WHEEL_URL_PREFIXES = (
    "https://files.pythonhosted.org/",
    "https://r.bgxiong.com/client/ai-story/",
)
MAX_ARCHIVE_BYTES = 512 * 1024 * 1024
MAX_UNCOMPRESSED_BYTES = 512 * 1024 * 1024
MAX_ENTRIES = 4096
WHEEL_LOCK_SCHEMA_VERSION = 1
DEFAULT_WHEEL_LOCK_PATH = "bgx-wheel-lock.json"
COMMIT_RE = re.compile(r"[0-9a-f]{40}")
SHA256_RE = re.compile(r"[0-9a-f]{64}")
FOLDER_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
NODE_ID_RE = re.compile(r"^[a-z0-9]+(?:\.[a-z0-9-]+)+$")
LICENSE_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9.+-]*$")
WINDOWS_RESERVED = {
    "CON", "PRN", "AUX", "NUL",
    *(f"COM{i}" for i in range(1, 10)),
    *(f"LPT{i}" for i in range(1, 10)),
}
LICENSE_FILENAMES = {
    "license", "license.md", "license.txt", "license.rst",
    "licence", "licence.md", "licence.txt", "licence.rst",
    "copying", "copying.md", "copying.txt", "copying.lesser",
    "unlicense", "unlicense.txt",
}
LICENSE_GRANT_MARKERS = (
    "permission is hereby granted",
    "apache license",
    "gnu general public license",
    "gnu lesser general public license",
    "gnu affero general public license",
    "bsd 2-clause",
    "bsd 3-clause",
    "redistribution and use",
    "mozilla public license",
    "isc license",
    "creative commons",
    "the unlicense",
    "spdx-license-identifier",
    "licensed under the",
    "this software is provided",
    "you may use, copy, modify",
)
CACHE_PARTS = {
    "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache",
    ".tox", ".nox", ".coverage", ".git",
}
CACHE_SUFFIXES = {".pyc", ".pyo", ".pyd"}
VENV_PARTS = {".venv", "venv", "site-packages"}
MODEL_SUFFIXES = {".safetensors", ".ckpt", ".onnx", ".gguf", ".pt", ".pth", ".h5"}
SECRET_EXACT = {
    ".env", ".netrc", ".pypirc", ".git-credentials", ".npmrc",
    "id_rsa", "id_dsa", "id_ecdsa", "id_ed25519",
    "credentials.json", "secrets.json", "service-account.json",
}
SECRET_SUFFIXES = {".pem", ".p12", ".pfx", ".kdbx"}
SECRET_PREFIXES = (".env.",)
ZIP_TIMESTAMP = (1980, 1, 1, 0, 0, 0)


class PackError(Exception):
    def __init__(self, message: str, exit_code: int = 1) -> None:
        super().__init__(message)
        self.exit_code = exit_code


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def normalize_dist_name(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).casefold()


def run_git(repo: Path, *args: str) -> bytes:
    completed = subprocess.run(
        ["git", "-C", str(repo), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.decode("utf-8", "replace").strip() or "git command failed"
        raise PackError(f"git {' '.join(args)} failed: {detail}")
    return completed.stdout


def require_commit(value: str) -> str:
    if not isinstance(value, str) or not COMMIT_RE.fullmatch(value):
        raise PackError(
            "gitCommit must be an explicit 40-character lowercase hex SHA; "
            "refusing to guess a commit from HEAD, a short hash, or a hand-installed directory"
        )
    return value


def reserved_device_name(segment: str) -> bool:
    stem = segment.split(".", 1)[0]
    return stem.upper() in WINDOWS_RESERVED


def validate_path_segment(segment: str, *, kind: str) -> str:
    if not segment or segment in {".", ".."} or "\\" in segment or ":" in segment:
        raise PackError(f"Unsafe {kind} segment: {segment!r}")
    if segment.endswith(".") or segment.endswith(" ") or segment.startswith(" "):
        raise PackError(f"Unsafe {kind} segment: {segment!r}")
    if reserved_device_name(segment):
        raise PackError(f"Reserved device name in {kind}: {segment!r}")
    return segment


def posix_relative_path(value: str, *, kind: str = "path") -> PurePosixPath:
    if not isinstance(value, str) or not value or "\\" in value or ":" in value:
        raise PackError(f"Invalid {kind}: {value!r}")
    if "\x00" in value or value.startswith("/") or value.startswith("./"):
        raise PackError(f"Unsafe {kind}: {value!r}")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in ("", ".", "..") for part in value.split("/")):
        raise PackError(f"Unsafe {kind}: {value!r}")
    for part in path.parts:
        validate_path_segment(part, kind=kind)
    return path


def validate_folder_name(value: str) -> str:
    if not isinstance(value, str) or not FOLDER_NAME_RE.fullmatch(value):
        raise PackError(
            "folderName must be a single ASCII directory name of letters, digits, "
            "dot, underscore or hyphen, starting with a letter or digit"
        )
    validate_path_segment(value, kind="folderName")
    return value


def validate_node_id(value: str) -> str:
    if not isinstance(value, str) or not NODE_ID_RE.fullmatch(value):
        raise PackError("id must be a lowercase ASCII reverse-domain identifier")
    return value


def validate_license_id(value: str) -> str:
    if not isinstance(value, str) or not LICENSE_ID_RE.fullmatch(value) or len(value) > 64:
        raise PackError("license must be a short SPDX-style identifier")
    return value


def validate_homepage(value: str) -> str:
    if not isinstance(value, str):
        raise PackError("homepage must be an HTTPS source URL")
    parsed = urlparse(value)
    if (
        parsed.scheme != "https"
        or parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
        or parsed.params
        or not parsed.netloc
        or not parsed.path
        or parsed.netloc.endswith(":")
        or ":" in parsed.netloc
    ):
        raise PackError(
            "homepage must be a plain HTTPS source URL without credentials, "
            "query, fragment or custom port"
        )
    return value


def validate_locked_url(value: str, *, kind: str) -> str:
    if not isinstance(value, str):
        raise PackError(f"Invalid {kind} URL")
    parsed = urlparse(value)
    if (
        parsed.scheme != "https"
        or parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
        or parsed.params
        or not parsed.netloc
        or not parsed.path
        or parsed.path.endswith("/")
        or ":" in parsed.netloc
    ):
        raise PackError(f"{kind} URL must be a precise HTTPS object without query, fragment, auth or custom port")
    if not value.startswith(ALLOWED_WHEEL_URL_PREFIXES):
        raise PackError(f"{kind} URL is not a locked production CDN prefix or files.pythonhosted.org object")
    return value


def classify_tree_path(rel: str) -> str:
    path = posix_relative_path(rel, kind="tree path")
    parts = {part.casefold() for part in path.parts}
    name = path.name
    folded = name.casefold()
    suffix = Path(name).suffix.casefold()
    if ".git" in parts:
        return "reject-git"
    if parts & {part.casefold() for part in VENV_PARTS}:
        return "reject-venv"
    if suffix in MODEL_SUFFIXES:
        return "reject-model"
    if folded in SECRET_EXACT or suffix in SECRET_SUFFIXES or any(folded.startswith(prefix) for prefix in SECRET_PREFIXES):
        return "reject-secret"
    if parts & {part.casefold() for part in CACHE_PARTS} or suffix in CACHE_SUFFIXES:
        return "exclude-cache"
    return "keep"


def is_license_file(rel: str) -> bool:
    path = PurePosixPath(rel)
    name = path.name.casefold()
    if name in LICENSE_FILENAMES:
        return True
    if name.startswith("license-") or name.startswith("licence-"):
        return True
    if len(path.parts) == 2 and path.parts[0].casefold() in {"licenses", "licences"}:
        return True
    return False


def license_allows_redistribution(data: bytes) -> bool:
    text = data.decode("utf-8", "replace")
    lowered = " ".join(text.lower().split())
    if any(marker in lowered for marker in LICENSE_GRANT_MARKERS):
        return True
    if "all rights reserved" in lowered and "permission" not in lowered and "redistribut" not in lowered:
        return False
    # Short SPDX-only or named license files still count if they are not a reservation.
    if re.search(r"\b(mit|apache-2\.0|bsd-2-clause|bsd-3-clause|gpl-2\.0|gpl-3\.0|lgpl-3\.0|mpl-2\.0|isc|unlicense|cc0-1\.0)\b", lowered):
        return True
    return False


def parse_requirement_names(text: str) -> list[str]:
    names: list[str] = []
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("-"):
            raise PackError(f"Unsupported requirements directive: {line}")
        if "://" in line or line.startswith("git+") or line.startswith("file:"):
            raise PackError(f"Requirements must not use VCS, URL or local path sources: {line}")
        token = re.split(r"[<>=!~;\[]", line, maxsplit=1)[0].strip()
        if not token or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", token):
            raise PackError(f"Cannot parse requirement name: {line}")
        names.append(token)
    return names


def pyproject_dependencies(data: bytes) -> list[str]:
    import tomllib

    try:
        parsed = tomllib.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise PackError(f"Invalid pyproject.toml: {exc}") from exc
    deps = parsed.get("project", {}).get("dependencies") or []
    if not isinstance(deps, list):
        raise PackError("pyproject.toml project.dependencies must be an array")
    names: list[str] = []
    for item in deps:
        if not isinstance(item, str):
            raise PackError("pyproject.toml dependencies must be strings")
        names.extend(parse_requirement_names(item))
    poetry = (parsed.get("tool") or {}).get("poetry") or {}
    poetry_deps = poetry.get("dependencies") or {}
    if isinstance(poetry_deps, dict):
        for name in poetry_deps:
            if name.casefold() != "python":
                names.append(str(name))
    return names


def detect_requirements(files: Mapping[str, bytes]) -> tuple[list[str], str | None]:
    names: list[str] = []
    requirements_path: str | None = None
    for candidate in ("requirements.txt", "requirements-in.txt"):
        if candidate in files:
            names.extend(parse_requirement_names(files[candidate].decode("utf-8", "replace")))
            requirements_path = candidate
            break
    if "pyproject.toml" in files:
        py_names = pyproject_dependencies(files["pyproject.toml"])
        names.extend(py_names)
        if py_names and requirements_path is None:
            requirements_path = "pyproject.toml"
    unique: list[str] = []
    seen: set[str] = set()
    for name in names:
        key = normalize_dist_name(name)
        if key not in seen:
            seen.add(key)
            unique.append(name)
    return unique, requirements_path


def validate_wheel_lock(data: bytes, required_names: Iterable[str]) -> dict:
    try:
        payload = json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise PackError(f"wheelLock is not UTF-8 JSON: {exc}") from exc
    if not isinstance(payload, dict) or payload.get("schemaVersion") != WHEEL_LOCK_SCHEMA_VERSION:
        raise PackError("wheelLock must be a JSON object with schemaVersion 1")
    if set(payload) != {"schemaVersion", "targets"}:
        raise PackError("wheelLock may only contain schemaVersion and targets")
    targets = payload.get("targets")
    if not isinstance(targets, list) or not targets:
        raise PackError("wheelLock must declare at least one Python/ABI/platform target")
    required = {normalize_dist_name(name) for name in required_names}
    seen_targets: set[tuple[str, str, str]] = set()
    for target in targets:
        if not isinstance(target, dict) or set(target) != {"pythonTag", "abiTag", "platformTag", "wheels"}:
            raise PackError("each wheelLock target must contain pythonTag, abiTag, platformTag and wheels")
        tags = []
        for key in ("pythonTag", "abiTag", "platformTag"):
            value = target[key]
            if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9._-]+", value):
                raise PackError(f"Invalid {key} in wheelLock")
            tags.append(value)
        tag_key = (tags[0], tags[1], tags[2])
        if tag_key in seen_targets:
            raise PackError(f"Duplicate wheelLock target: {'-'.join(tag_key)}")
        seen_targets.add(tag_key)
        wheels = target["wheels"]
        if not isinstance(wheels, list) or not wheels:
            raise PackError("wheelLock target is missing its dependency closure")
        present: set[str] = set()
        for wheel in wheels:
            if not isinstance(wheel, dict):
                raise PackError("wheelLock wheel entries must be objects")
            required_keys = {"name", "requires", "version", "filename", "bytes", "sha256", "url"}
            if set(wheel) != required_keys:
                raise PackError("wheelLock wheel entries must be a closed set of name, requires, version, filename, bytes, sha256, url")
            name = wheel["name"]
            if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", name):
                raise PackError(f"Invalid wheel name: {name!r}")
            key = normalize_dist_name(name)
            if key in present:
                raise PackError(f"Duplicate wheel name in target: {name}")
            present.add(key)
            if not isinstance(wheel["requires"], str) or not wheel["requires"].strip():
                raise PackError(f"wheel {name} is missing its PEP 440 requires value")
            if not isinstance(wheel["version"], str) or not wheel["version"].strip():
                raise PackError(f"wheel {name} is missing a locked version")
            filename = wheel["filename"]
            if not isinstance(filename, str) or "/" in filename or "\\" in filename or not filename.endswith(".whl"):
                raise PackError(f"wheel filename must be a single-segment .whl name: {filename!r}")
            posix_relative_path(filename, kind="wheel filename")
            if not isinstance(wheel["bytes"], int) or isinstance(wheel["bytes"], bool) or wheel["bytes"] <= 0:
                raise PackError(f"wheel {name} bytes must be a positive integer")
            if not isinstance(wheel["sha256"], str) or not SHA256_RE.fullmatch(wheel["sha256"]):
                raise PackError(f"wheel {name} sha256 must be 64 lowercase hex characters")
            validate_locked_url(wheel["url"], kind=f"wheel {name}")
        missing = required - present
        if missing:
            raise PackError("wheelLock does not cover declared requirements: " + ", ".join(sorted(missing)))
    return payload


def verify_source_repo(repo: Path, commit: str) -> Path:
    repo = repo.resolve()
    if not repo.exists():
        raise PackError(f"source repo does not exist: {repo}")
    if not repo.is_dir():
        raise PackError(f"source repo is not a directory: {repo}")
    try:
        run_git(repo, "rev-parse", "--git-dir")
    except PackError as exc:
        raise PackError(
            "source path is not a git repository; refusing to guess a commit "
            "from a hand-installed or plain directory"
        ) from exc
    try:
        resolved = run_git(repo, "rev-parse", "--verify", "--end-of-options", f"{commit}^{{commit}}").strip().decode("ascii").casefold()
    except PackError as exc:
        raise PackError(
            "source repository does not contain the declared commit; "
            "refusing to guess another commit"
        ) from exc
    if resolved != commit:
        raise PackError(f"declared commit {commit} resolved to a different object {resolved}")
    inside = run_git(repo, "rev-parse", "--is-inside-work-tree").strip() == b"true"
    if inside:
        status = run_git(repo, "status", "--porcelain=v1", "--untracked-files=all")
        if status.strip():
            raise PackError("source worktree is dirty; refusing to pack a dirty or uncommitted tree")
    return repo


def read_commit_blobs(repo: Path, commit: str) -> list[tuple[str, str, bytes]]:
    raw = run_git(repo, "ls-tree", "-r", "-z", "--full-tree", commit)
    entries: list[tuple[str, str]] = []
    if raw:
        parts = raw.split(b"\0")
        if parts and parts[-1] == b"":
            parts = parts[:-1]
        for item in parts:
            try:
                meta, path_bytes = item.split(b"\t", 1)
            except ValueError as exc:
                raise PackError("invalid git ls-tree output") from exc
            fields = meta.decode("ascii").split()
            if len(fields) != 3:
                raise PackError("invalid git ls-tree metadata")
            mode, kind, digest = fields
            rel = path_bytes.decode("utf-8")
            posix_relative_path(rel, kind="tree path")
            if kind != "blob" or mode in {"120000", "160000"}:
                raise PackError(f"refusing symlink, gitlink or non-blob path in commit: {rel}")
            if not re.fullmatch(r"[0-9a-f]{40}", digest):
                raise PackError(f"unexpected git object id for {rel}")
            entries.append((rel, digest))
    if not entries:
        raise PackError("declared commit has no files")
    request = b"".join(f"{digest}\n".encode("ascii") for _, digest in entries)
    completed = subprocess.run(
        ["git", "-C", str(repo), "cat-file", "--batch"],
        input=request,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        raise PackError("git cat-file --batch failed: " + completed.stderr.decode("utf-8", "replace").strip())
    blobs: dict[str, bytes] = {}
    cursor = 0
    output = completed.stdout
    while cursor < len(output):
        nl = output.find(b"\n", cursor)
        if nl < 0:
            raise PackError("truncated git cat-file output")
        header = output[cursor:nl].decode("ascii")
        cursor = nl + 1
        fields = header.split()
        if len(fields) == 2 and fields[1] == "missing":
            raise PackError(f"missing git blob {fields[0]}")
        if len(fields) != 3 or fields[1] != "blob":
            raise PackError(f"unexpected git cat-file header: {header}")
        digest, _, size_text = fields
        try:
            size = int(size_text)
        except ValueError as exc:
            raise PackError("invalid git blob size") from exc
        data = output[cursor:cursor + size]
        if len(data) != size:
            raise PackError("truncated git blob")
        cursor += size
        if cursor < len(output) and output[cursor] == 10:
            cursor += 1
        else:
            raise PackError("git cat-file record was not newline-terminated")
        blobs[digest] = data
    result: list[tuple[str, str, bytes]] = []
    for rel, digest in entries:
        if digest not in blobs:
            raise PackError(f"blob for {rel} was not returned by git")
        result.append((rel, digest, blobs[digest]))
    return result


def select_source_files(entries: list[tuple[str, str, bytes]]) -> tuple[dict[str, bytes], list[dict[str, str]]]:
    kept: dict[str, bytes] = {}
    excluded: list[dict[str, str]] = []
    folded: dict[str, str] = {}
    for rel, _digest, data in entries:
        action = classify_tree_path(rel)
        if action == "exclude-cache":
            excluded.append({"path": rel, "reason": "cache"})
            continue
        if action == "reject-git":
            raise PackError(f"commit contains Git history path {rel}; refusing to pack")
        if action == "reject-venv":
            raise PackError(f"commit contains a virtualenv path {rel}; refusing to pack")
        if action == "reject-model":
            raise PackError(f"commit contains model weights {rel}; refusing to mix models into the node archive")
        if action == "reject-secret":
            raise PackError(f"commit contains a secret or credential file {rel}; refusing to pack")
        key = rel.casefold()
        if key in folded:
            raise PackError(f"case-conflicting paths in commit: {folded[key]} and {rel}")
        folded[key] = rel
        kept[rel] = data
    if not kept:
        raise PackError("no source files remain after applying exclusion rules")
    return kept, excluded


def collect_license_files(files: Mapping[str, bytes]) -> list[dict[str, str]]:
    licenses = []
    for rel, data in sorted(files.items()):
        if not is_license_file(rel):
            continue
        if not license_allows_redistribution(data):
            raise PackError(f"license material does not permit redistribution: {rel}")
        licenses.append({"path": rel, "sha256": sha256_hex(data)})
    if not licenses:
        raise PackError("no license material found in the declared commit; refusing to ship")
    return licenses


def archive_member_path(folder_name: str, rel: str, strip_components: int) -> str:
    inner = posix_relative_path(rel, kind="archive path").as_posix()
    if strip_components == 0:
        return inner
    return f"{folder_name}/{inner}"


def write_deterministic_zip(members: list[tuple[str, bytes]]) -> bytes:
    if len(members) > MAX_ENTRIES:
        raise PackError(f"archive would contain more than {MAX_ENTRIES} entries")
    uncompressed = sum(len(data) for _, data in members)
    if uncompressed > MAX_UNCOMPRESSED_BYTES:
        raise PackError("uncompressed archive contents exceed the 512 MiB limit")
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path, data in members:
            info = zipfile.ZipInfo(path, date_time=ZIP_TIMESTAMP)
            info.create_system = 3
            info.external_attr = (0o644 & 0o777) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            if not path.isascii():
                info.flag_bits |= 0x800
            archive.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    payload = buffer.getvalue()
    if len(payload) > MAX_ARCHIVE_BYTES:
        raise PackError("compressed archive exceeds the 512 MiB download limit")
    return payload


def build_members(
    folder_name: str,
    strip_components: int,
    files: Mapping[str, bytes],
    companions: Mapping[str, bytes],
) -> list[tuple[str, bytes]]:
    folded: dict[str, str] = {}
    members: list[tuple[str, bytes]] = []
    for rel, data in {**files, **companions}.items():
        path = archive_member_path(folder_name, rel, strip_components)
        key = path.casefold()
        if key in folded:
            raise PackError(f"case-conflicting archive paths: {folded[key]} and {path}")
        folded[key] = path
        members.append((path, data))
    members.sort(key=lambda item: item[0].encode("utf-8"))
    if strip_components == 1:
        roots = {PurePosixPath(path).parts[0] for path, _ in members}
        if roots != {folder_name}:
            raise PackError("stripComponents=1 archive must have a single root directory named folderName")
    return members


def expected_archive_url(folder_name: str, commit: str) -> str:
    return f"{PRODUCTION_ARCHIVE_BASE}/{folder_name}/{commit}.zip"


def load_wheel_lock(path: Path) -> bytes:
    if not path.is_file():
        raise PackError(f"wheel lock file does not exist: {path}")
    return path.read_bytes()


def pack_archive(
    *,
    source_repo: Path,
    git_commit: str,
    folder_name: str,
    node_id: str,
    homepage: str,
    license_id: str,
    output_dir: Path,
    display_name: str | None = None,
    strip_components: int = 1,
    wheel_lock_file: Path | None = None,
    wheel_lock_path: str = DEFAULT_WHEEL_LOCK_PATH,
    requirements_path: str | None = None,
    notes: str | None = None,
) -> dict:
    commit = require_commit(git_commit)
    folder_name = validate_folder_name(folder_name)
    node_id = validate_node_id(node_id)
    homepage = validate_homepage(homepage)
    license_id = validate_license_id(license_id)
    if strip_components not in (0, 1):
        raise PackError("stripComponents may only be 0 or 1")
    repo = verify_source_repo(source_repo, commit)
    entries = read_commit_blobs(repo, commit)
    files, excluded = select_source_files(entries)
    license_files = collect_license_files(files)
    detected_names, detected_requirements = detect_requirements(files)
    if requirements_path is not None:
        rel = posix_relative_path(requirements_path, kind="requirements path").as_posix()
        if rel not in files:
            raise PackError(f"requirements path is not in the declared commit tree: {rel}")
        if rel.endswith(".toml"):
            required_names = pyproject_dependencies(files[rel]) if not detected_names else detected_names
        else:
            required_names = parse_requirement_names(files[rel].decode("utf-8", "replace"))
        requirements_rel = rel
    else:
        required_names = detected_names
        requirements_rel = detected_requirements

    companions: dict[str, bytes] = {}
    requirements_block = None
    if required_names and wheel_lock_file is None:
        raise PackError(
            "declared or detected dependencies require a wheel lock with Python tags "
            "and a complete dependency closure; refusing to ship"
        )
    if wheel_lock_file is not None:
        lock_rel = posix_relative_path(wheel_lock_path, kind="wheelLock path").as_posix()
        if lock_rel.casefold() in {path.casefold() for path in files}:
            raise PackError(f"wheelLock path collides with upstream source: {lock_rel}")
        lock_bytes = load_wheel_lock(wheel_lock_file)
        validate_wheel_lock(lock_bytes, required_names)
        if not required_names:
            raise PackError("a wheel lock was supplied but no requirements were declared or detected")
        if requirements_rel is None:
            raise PackError("dependencies require an in-archive requirements path")
        companions[lock_rel] = lock_bytes
        requirements_block = {
            "path": requirements_rel,
            "sha256": sha256_hex(files[requirements_rel]),
            "wheelLock": lock_rel,
        }
    elif wheel_lock_file is None and not required_names:
        requirements_block = None

    members = build_members(folder_name, strip_components, files, companions)
    payload = write_deterministic_zip(members)

    output_dir = output_dir.resolve()
    target_dir = output_dir / folder_name
    zip_name = f"{commit}.zip"
    zip_path = target_dir / zip_name
    manifest_path = target_dir / f"{commit}.manifest.json"
    target_dir.mkdir(parents=True, exist_ok=True)

    digest = sha256_hex(payload)
    modifications = []
    for rel in companions:
        modifications.append(f"added companion {rel}")
    if excluded:
        modifications.append("excluded generated cache paths from the packed tree")
    if notes:
        modifications.append(notes)

    manifest = {
        "schemaVersion": 1,
        "kind": "comfyCustomNodeArchive",
        "id": node_id,
        "folderName": folder_name,
        "displayName": display_name or folder_name,
        "license": license_id,
        "homepage": homepage,
        "gitCommit": commit,
        "archive": {
            "url": expected_archive_url(folder_name, commit),
            "sha256": digest,
            "bytes": len(payload),
            "stripComponents": strip_components,
            "fileName": zip_name,
        },
        "licenseFiles": license_files,
        "upstreamFiles": [
            {"path": rel, "sha256": sha256_hex(data), "bytes": len(data)}
            for rel, data in sorted(files.items())
        ],
        "companions": [
            {"path": rel, "sha256": sha256_hex(data), "bytes": len(data), "role": "wheelLock"}
            for rel, data in sorted(companions.items())
        ],
        "excluded": excluded,
        "sourceNotes": {
            "gitCommit": commit,
            "homepage": homepage,
            "modifications": modifications,
        },
    }
    if requirements_block is not None:
        manifest["requirements"] = requirements_block
    else:
        manifest["requirements"] = None

    encoded_manifest = (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode("utf-8")

    if zip_path.exists() and zip_path.read_bytes() != payload:
        raise PackError(f"refusing to overwrite an existing archive with different bytes: {zip_path}")
    if manifest_path.exists() and manifest_path.read_bytes() != encoded_manifest:
        raise PackError(f"refusing to overwrite an existing manifest with different bytes: {manifest_path}")

    fd, tmp_name = tempfile.mkstemp(prefix=f".{commit}.", suffix=".zip.part", dir=str(target_dir))
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(tmp_name, zip_path)
        manifest_tmp = manifest_path.with_suffix(".json.part")
        manifest_tmp.write_bytes(encoded_manifest)
        os.replace(manifest_tmp, manifest_path)
    finally:
        if os.path.exists(tmp_name):
            os.unlink(tmp_name)
        leftover = manifest_path.with_suffix(".json.part")
        if leftover.exists():
            leftover.unlink()

    # Confirm written bytes still match the recorded digest; never invent one.
    written = zip_path.read_bytes()
    if sha256_hex(written) != digest or len(written) != len(payload):
        raise PackError("written archive digest does not match the recorded sha256")
    manifest["archive"]["path"] = str(zip_path)
    manifest["manifestPath"] = str(manifest_path)
    return manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-repo", type=Path, required=True, help="git repository containing the declared commit")
    parser.add_argument("--git-commit", required=True, help="explicit 40-character lowercase hex commit SHA")
    parser.add_argument("--folder-name", required=True, help="install directory and CDN path segment")
    parser.add_argument("--id", required=True, dest="node_id", help="lowercase reverse-domain node id")
    parser.add_argument("--homepage", required=True, help="HTTPS source homepage for display only")
    parser.add_argument("--license", required=True, dest="license_id", help="SPDX-style license identifier")
    parser.add_argument("--output-dir", type=Path, required=True, help="directory that receives <folderName>/<commit>.zip")
    parser.add_argument("--display-name", default=None)
    parser.add_argument("--strip-components", type=int, default=1, choices=(0, 1))
    parser.add_argument("--wheel-lock", type=Path, default=None, help="existing wheel lock JSON to add as a companion file")
    parser.add_argument("--wheel-lock-path", default=DEFAULT_WHEEL_LOCK_PATH, help="relative path of the companion inside the node root")
    parser.add_argument("--requirements-path", default=None, help="in-tree requirements file relative to the source root")
    parser.add_argument("--notes", default=None, help="optional modification note recorded in the sidecar manifest")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        manifest = pack_archive(
            source_repo=args.source_repo,
            git_commit=args.git_commit,
            folder_name=args.folder_name,
            node_id=args.node_id,
            homepage=args.homepage,
            license_id=args.license_id,
            output_dir=args.output_dir,
            display_name=args.display_name,
            strip_components=args.strip_components,
            wheel_lock_file=args.wheel_lock,
            wheel_lock_path=args.wheel_lock_path,
            requirements_path=args.requirements_path,
            notes=args.notes,
        )
        print(json.dumps(manifest, ensure_ascii=False, indent=2))
        return 0
    except PackError as exc:
        print(f"custom node archive pack failed: {exc}", file=sys.stderr)
        return exc.exit_code
    except OSError as exc:
        print(f"custom node archive pack failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
