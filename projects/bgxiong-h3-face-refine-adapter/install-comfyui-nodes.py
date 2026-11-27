#!/usr/bin/env python3
"""Install the server nodes from the extracted face refinement adapter package."""

import argparse
from contextlib import contextmanager
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import sys
import tempfile
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import urlopen
import uuid


NODE_NAMES = ("ComfyUI-H3-FaceRefine", "BGXFaceRefineContract")
SCENE_WHEEL = Path(
    "BGXFaceRefineContract/dependencies/scenedetect-0.7.3-py3-none-any.whl"
)
CORE_MODULES = ("aiohttp", "av", "numpy", "PIL", "torch", "ultralytics")
PACKAGE_ROOT = Path(__file__).resolve().parent


def is_link(path):
    attrs = path.lstat()
    return stat.S_ISLNK(attrs.st_mode) or bool(
        getattr(attrs, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
    )


def read_json_file(path):
    if is_link(path) or not path.is_file():
        raise ValueError(f"需要常规文件：{path}")
    return json.loads(path.read_text(encoding="utf-8-sig"))


def directory_files(root, ignore_cache=False):
    """Return regular file paths relative to root. Reject link and junction entries."""
    if not root.is_dir() or is_link(root):
        raise ValueError(f"需要常规目录：{root}")
    found = set()
    for entry in root.iterdir():
        if is_link(entry):
            raise ValueError(f"节点目录内含链接或挂载点：{entry}")
        if ignore_cache and (entry.name == "__pycache__" or entry.suffix == ".pyc"):
            continue
        if entry.is_dir():
            found.update(Path(entry.name) / child for child in directory_files(entry, ignore_cache))
        elif entry.is_file():
            found.add(Path(entry.name))
        else:
            raise ValueError(f"节点目录内含非常规文件：{entry}")
    return found


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_tree(root, expected, ignore_cache=False):
    actual = directory_files(root, ignore_cache=ignore_cache)
    if actual != set(expected):
        missing = sorted(str(path) for path in set(expected) - actual)
        extra = sorted(str(path) for path in actual - set(expected))
        raise ValueError(f"节点文件清单不一致：{root}；缺少 {missing}；多出 {extra}")
    for path, sha256 in expected.items():
        if digest(root / path) != sha256:
            raise ValueError(f"节点文件 SHA-256 不一致：{root / path}")


def package_inventory(package):
    envelope = read_json_file(package / "adapter-install.json")
    if is_link(package / "package"):
        raise ValueError("适配器包运行目录不得是链接")
    manifest_path = package / "package" / "manifest.json"
    manifest = read_json_file(manifest_path)
    if (manifest.get("pluginId") != "local.thirdparty.h3-face-refine"
            or digest(manifest_path) != envelope.get("manifestSha256")):
        raise ValueError("小脸精修适配器的清单与安装信封不匹配")
    binding = envelope.get("comfyui", {})
    if binding.get("customNodes") != "comfy-nodes" or binding.get("serviceOnly"):
        raise ValueError("适配器包缺少内置 ComfyUI 节点")

    inventory = {}
    used_names = set()
    for item in binding["files"]:
        raw = item["path"]
        fields = raw.split("/")
        if (len(fields) < 3 or fields[0] != "comfy-nodes" or fields[1] not in NODE_NAMES
                or any(part in ("", ".", "..") or not re.fullmatch(r"[A-Za-z0-9_.-]+", part)
                       for part in fields[2:])):
            raise ValueError(f"节点文件路径越界或无效：{raw}")
        path = Path(*PurePosixPath(raw).parts[1:])
        sha256 = item["sha256"]
        if raw.casefold() in used_names or not re.fullmatch(r"[0-9a-f]{64}", sha256):
            raise ValueError(f"节点文件路径重复或摘要无效：{raw}")
        used_names.add(raw.casefold())
        inventory[path] = sha256
    if SCENE_WHEEL not in inventory or any(
        Path(name, "__init__.py") not in inventory for name in NODE_NAMES
    ):
        raise ValueError("适配器包里的节点组件或自动切镜依赖不完整")
    verify_tree(package / "comfy-nodes", inventory)
    return inventory


def resolve_target(root, custom_nodes):
    root = root.resolve(strict=True)
    if not (root / "main.py").is_file() or not (root / "folder_paths.py").is_file():
        raise ValueError(f"请选择包含 main.py 和 folder_paths.py 的 ComfyUI 目录：{root}")
    target = custom_nodes if custom_nodes is not None else root / "custom_nodes"
    # A caller can specify an alternative custom_nodes path set by --base-directory.
    # The chosen path itself must be a real directory or be created by ComfyUI first.
    if not target.is_dir() or is_link(target):
        raise ValueError(f"请选择 ComfyUI 实际读取的常规 custom_nodes 目录：{target}")
    target = target.resolve(strict=True)
    return target


def check_server_python():
    if sys.version_info < (3, 11):
        raise ValueError(f"节点需要 Python 3.11 或更高版本：{sys.executable}")
    missing = [name for name in CORE_MODULES if importlib.util.find_spec(name) is None]
    if missing:
        raise ValueError(
            f"当前 Python 缺少视频分析依赖：{', '.join(missing)}。"
            f"请使用目标 ComfyUI 的 Python，并在该环境补齐依赖。当前解释器：{sys.executable}"
        )


def installed(target, nodes):
    check = {}
    existing_names = {path.name.casefold(): path for path in target.iterdir()}
    for name, inventory in nodes.items():
        current = target / name
        alias = existing_names.get(name.casefold())
        if alias is not None and alias.name != name:
            raise ValueError(f"检测到与节点目录同名的大小写变体，请核对：{alias}")
        if current.exists() or current.is_symlink():
            if is_link(current) or not current.is_dir():
                raise ValueError(f"现有节点位置不是可备份的常规目录：{current}")
            try:
                verify_tree(current, inventory, ignore_cache=True)
            except ValueError:
                check[name] = "replace"
            else:
                check[name] = "current"
        else:
            check[name] = "new"
    return check


@contextmanager
def target_install_lock(path):
    if path.is_symlink() or (path.exists() and is_link(path)):
        raise ValueError(f"节点安装锁不得是链接：{path}")
    # OS-owned locks are released after a crash. Keep a stable filename so
    # competing installers never open two distinct locks for the same target.
    with path.open("a+b") as stream:
        try:
            if os.name == "nt":
                import msvcrt

                stream.seek(0)
                msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl

                fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            raise ValueError(f"这个节点目录正在被另一个安装进程更新：{path}") from error
        yield


def node_inventories(inventory):
    return {
        name: {Path(*path.parts[1:]): sha for path, sha in inventory.items()
               if path.parts[0] == name}
        for name in NODE_NAMES
    }


def install(source, target, nodes):
    work = target.parent / ".bgx-face-refine-backups"
    work.mkdir(exist_ok=True)
    if not work.is_dir() or is_link(work) or work.stat().st_dev != target.stat().st_dev:
        raise ValueError(f"节点备份目录须为与 custom_nodes 同卷的常规目录：{work}")
    with target_install_lock(work / "install.lock"):
        actions = installed(target, nodes)
        changed = [name for name in NODE_NAMES if actions[name] != "current"]
        if not changed:
            return None
        with tempfile.TemporaryDirectory(prefix="stage-", dir=work) as staging:
            stage = Path(staging)
            for name in changed:
                shutil.copytree(source / name, stage / name)
                verify_tree(stage / name, nodes[name])

            backup = work / str(uuid.uuid4()) if "replace" in actions.values() else None
            if backup:
                backup.mkdir()
                # The location remains visible if the machine powers off mid-install.
                print(f"现有节点备份目录：{backup}", flush=True)
            saved = []
            moved = []
            try:
                for name in changed:
                    if actions[name] == "replace":
                        (target / name).rename(backup / name)
                        saved.append(name)
                    (stage / name).rename(target / name)
                    moved.append(name)
                for name, files in nodes.items():
                    verify_tree(target / name, files, ignore_cache=True)
            except BaseException:
                restore = [(target / name, stage / name) for name in reversed(moved)]
                restore += [(backup / name, target / name) for name in reversed(saved)]
                errors = []
                for previous, destination in restore:
                    try:
                        previous.rename(destination)
                    except OSError as error:
                        errors.append(str(error))
                if errors:
                    recovery = backup or work
                    raise OSError(f"节点回滚未完成，请保留备份目录 {recovery}：{', '.join(errors)}")
                raise
            return backup


def endpoint_json(base, route):
    try:
        with urlopen(base + route, timeout=10) as response:
            return json.load(response)
    except HTTPError as error:
        raise ValueError(f"{base + route} 返回 HTTP {error.code}") from error


def verify_service(url):
    parsed = urlsplit(url)
    if (parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username
            or parsed.password or parsed.query or parsed.fragment):
        raise ValueError("请输入 ComfyUI 的 HTTP 或 HTTPS 基址")
    base = url.rstrip("/")
    health = endpoint_json(base, "/bgx_face_refine/health")
    capability_flags = ("queuedAnalysis", "analysisSessions", "trackingReceipt")
    if not isinstance(health, dict) or not all(health.get(flag) is True for flag in capability_flags):
        raise ValueError(f"{base} 未加载当前小脸精修的排队分析接口")
    for name in ("BGXFaceAnalyze", "BGXFaceTrack", "H3FaceSelect"):
        if name not in endpoint_json(base, "/object_info/" + name):
            raise ValueError(f"{base} 未注册节点 {name}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--check", action="store_true", help="核对包和目标环境，预览安装操作")
    action.add_argument("--install", action="store_true", help="在已经停止的 ComfyUI 安装节点")
    action.add_argument("--verify-url", metavar="BASE_URL", help="检查启动后的 ComfyUI 是否已加载本包")
    parser.add_argument("--comfy-root", type=Path, help="目标 ComfyUI 的 main.py 所在目录")
    parser.add_argument("--custom-nodes", type=Path, help="指定目标实例通过参数配置的 custom_nodes 路径")
    args = parser.parse_args()
    try:
        if args.verify_url:
            verify_service(args.verify_url)
            print(f"ComfyUI 已加载小脸精修排队分析、跟踪及选脸节点：{args.verify_url}")
            return 0
        if args.comfy_root is None:
            parser.error("--check / --install 需要 --comfy-root")
        inventory = package_inventory(PACKAGE_ROOT)
        nodes = node_inventories(inventory)
        target = resolve_target(args.comfy_root, args.custom_nodes)
        if target.is_relative_to(PACKAGE_ROOT) or PACKAGE_ROOT.is_relative_to(target):
            raise ValueError("适配器解压目录与 ComfyUI 的 custom_nodes 必须各自独立")
        check_server_python()
        if args.check:
            labels = {"new": "待安装", "replace": "待备份并更新", "current": "已是当前版本"}
            print(f"节点目录：{target}")
            for name, state in installed(target, nodes).items():
                print(f"{name}：{labels[state]}")
        else:
            install(PACKAGE_ROOT / "comfy-nodes", target, nodes)
            print(f"节点就绪：{target}。按原方式启动 ComfyUI，然后用 --verify-url 验证地址。")
        return 0
    except (OSError, KeyError, TypeError, ValueError) as error:
        print(f"小脸精修节点准备失败：{error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
