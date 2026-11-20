#!/usr/bin/env python3
"""Run the official first-install QA against an already-built host test binary.

The official wrapper always invokes `cargo test`, which would recompile the
currently uncompilable dirty host tree. This helper reproduces the same
signed loopback index and environment, then launches the existing test exe.
"""
from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
import tempfile
import importlib.util

HOST_ROOT = Path(r"L:\00Dev\bgxiong-ai-story")
SCRIPT = HOST_ROOT / "scripts" / "test-adapter-online-first-install.py"
DEFAULT_BIN = HOST_ROOT / "src-tauri" / "target" / "debug" / "deps" / "bgxiong_ai_story_tauri-b5c172c39ee22560.exe"


def load_official():
    spec = importlib.util.spec_from_file_location("official_first_install", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--video-models", required=True, type=int)
    parser.add_argument("--bin", type=Path, default=DEFAULT_BIN)
    args = parser.parse_args()
    official = load_official()
    archive_src = args.archive.resolve()
    binary = args.bin.resolve()
    if not archive_src.is_file():
        raise SystemExit(f"archive missing: {archive_src}")
    if not binary.is_file():
        raise SystemExit(f"test binary missing: {binary}")

    with tempfile.TemporaryDirectory(prefix="bgx-adapter-first-install-") as temporary:
        qa = Path(temporary)
        archive = qa / archive_src.name
        shutil.copyfile(archive_src, archive)
        index, trust = official.signed_index(archive)
        (qa / "latest.json").write_text(index, encoding="utf-8")
        handler = partial(SimpleHTTPRequestHandler, directory=str(qa))
        with ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
            thread = Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                env = {name: value for name, value in os.environ.items()
                       if not name.upper().startswith(("TAURI_SIGNING_", "BGX_ADAPTER_INDEX_KEY_"))}
                debug_dir = str(HOST_ROOT / "src-tauri" / "target" / "debug")
                deps_dir = str(HOST_ROOT / "src-tauri" / "target" / "debug" / "deps")
                env["PATH"] = os.pathsep.join([deps_dir, debug_dir, env.get("PATH", "")])
                env["CARGO_MANIFEST_DIR"] = str(HOST_ROOT / "src-tauri")
                env.update(
                    BGX_ADAPTER_ONLINE_INDEX_URL=f"http://127.0.0.1:{server.server_port}/latest.json",
                    BGX_ADAPTER_ONLINE_TRUST=trust,
                    BGX_ADAPTER_ONLINE_QA_VIDEO_MODELS=str(args.video_models),
                )
                cmd = [
                    str(binary),
                    "--ignored",
                    "--nocapture",
                    "--exact",
                    "comfyui::workflow::adapter_online_update::apply::state_machine_tests::real_online_first_install",
                ]
                print("running:", cmd, "cwd=", HOST_ROOT / "src-tauri", flush=True)
                completed = subprocess.run(
                    cmd,
                    cwd=str(HOST_ROOT / "src-tauri"),
                    env=env,
                    check=False,
                )
                return completed.returncode
            finally:
                server.shutdown()
                thread.join()


if __name__ == "__main__":
    raise SystemExit(main())
