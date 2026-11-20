#!/usr/bin/env python3
"""Offline unit tests for pack-custom-node-archive.py. No network, no real upstream commit."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path

SCRIPT = Path(__file__).with_name("pack-custom-node-archive.py")
spec = importlib.util.spec_from_file_location("pack_custom_node_archive", SCRIPT)
pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pack)

MIT_LICENSE = b"""MIT License

Copyright (c) 2026 Example Authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files.
"""

ARR_LICENSE = b"Copyright 2026 Example. All rights reserved.\n"


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(repo: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        check=check,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def write_file(root: Path, rel: str, data: bytes) -> None:
    path = root.joinpath(*rel.split("/"))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def init_repo(root: Path, files: dict[str, bytes], dirty: dict[str, bytes] | None = None) -> str:
    root.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "init", "-b", "main", str(root)], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git(root, "config", "user.name", "Pack Test")
    git(root, "config", "user.email", "pack-test@example.com")
    git(root, "config", "core.autocrlf", "false")
    git(root, "config", "commit.gpgsign", "false")
    for rel, data in files.items():
        write_file(root, rel, data)
    git(root, "add", "-A")
    git(root, "commit", "-m", "seed")
    commit = git(root, "rev-parse", "HEAD").stdout.decode("ascii").strip()
    if dirty:
        for rel, data in dirty.items():
            write_file(root, rel, data)
    return commit


def valid_wheel_lock(names: list[str] | None = None) -> bytes:
    names = names or ["example-dep"]
    wheels = []
    for name in names:
        filename = f"{name.replace('-', '_')}-1.0.0-py3-none-any.whl"
        wheels.append({
            "name": name,
            "requires": "==1.0.0",
            "version": "1.0.0",
            "filename": filename,
            "bytes": 12,
            "sha256": "ab" * 32,
            "url": f"https://files.pythonhosted.org/packages/{filename}",
        })
    payload = {
        "schemaVersion": 1,
        "targets": [{
            "pythonTag": "cp313",
            "abiTag": "cp313",
            "platformTag": "win_amd64",
            "wheels": wheels,
        }],
    }
    return (json.dumps(payload, indent=2) + "\n").encode("utf-8")


def pack_kwargs(repo: Path, commit: str, output: Path, **overrides):
    values = dict(
        source_repo=repo,
        git_commit=commit,
        folder_name="ExampleNode",
        node_id="org.example.node",
        homepage="https://github.com/example/ExampleNode",
        license_id="MIT",
        output_dir=output,
        display_name="Example Node",
        strip_components=1,
    )
    values.update(overrides)
    return values


class PackCustomNodeArchiveTests(unittest.TestCase):
    def test_identical_inputs_yield_identical_bytes_and_extractable_source(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            files = {
                "LICENSE": MIT_LICENSE,
                "README.md": b"# Example Node\n",
                "__init__.py": b"NODE_CLASS_MAPPINGS = {}\n",
                "notes/source.txt": b"upstream source notes\n",
            }
            repo = base / "repo"
            commit = init_repo(repo, files)
            first_dir = base / "out1"
            second_dir = base / "out2"
            first = pack.pack_archive(**pack_kwargs(repo, commit, first_dir))
            second = pack.pack_archive(**pack_kwargs(repo, commit, second_dir))
            zip_a = (first_dir / "ExampleNode" / f"{commit}.zip").read_bytes()
            zip_b = (second_dir / "ExampleNode" / f"{commit}.zip").read_bytes()
            self.assertEqual(zip_a, zip_b)
            self.assertEqual(first["archive"]["sha256"], sha(zip_a))
            self.assertEqual(first["archive"]["bytes"], len(zip_a))
            self.assertEqual(first["archive"]["sha256"], second["archive"]["sha256"])
            self.assertEqual(
                first["archive"]["url"],
                f"https://r.bgxiong.com/client/ai-story/comfy-custom-nodes/ExampleNode/{commit}.zip",
            )
            self.assertEqual(first["archive"]["stripComponents"], 1)
            self.assertIsNone(first["requirements"])
            with zipfile.ZipFile(first_dir / "ExampleNode" / f"{commit}.zip") as archive:
                names = archive.namelist()
                self.assertEqual(names, sorted(names))
                for info in archive.infolist():
                    self.assertEqual(info.date_time, (1980, 1, 1, 0, 0, 0))
                    self.assertEqual(info.create_system, 3)
                    self.assertEqual(info.external_attr, 0o644 << 16)
                extracted = {name: archive.read(name) for name in names}
            self.assertEqual(extracted["ExampleNode/LICENSE"], MIT_LICENSE)
            self.assertEqual(extracted["ExampleNode/__init__.py"], files["__init__.py"])
            self.assertEqual(extracted["ExampleNode/notes/source.txt"], files["notes/source.txt"])
            self.assertEqual(set(extracted), {f"ExampleNode/{rel}" for rel in files})

    def test_idempotent_rerun_on_same_output_does_not_change_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n"})
            output = base / "out"
            first = pack.pack_archive(**pack_kwargs(repo, commit, output))
            raw = (output / "ExampleNode" / f"{commit}.zip").read_bytes()
            second = pack.pack_archive(**pack_kwargs(repo, commit, output))
            self.assertEqual(raw, (output / "ExampleNode" / f"{commit}.zip").read_bytes())
            self.assertEqual(first["archive"]["sha256"], second["archive"]["sha256"])

    def test_refuses_missing_short_uppercase_or_symbolic_commit(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n"})
            output = base / "out"
            for value in ("", "HEAD", commit[:7], commit.upper(), "not-a-commit"):
                with self.subTest(value=value):
                    with self.assertRaisesRegex(pack.PackError, "explicit 40-character lowercase hex"):
                        pack.pack_archive(**pack_kwargs(repo, value, output))
            self.assertFalse(any(output.rglob("*.zip")))

    def test_refuses_non_git_hand_installed_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            hand = base / "ComfyUI" / "custom_nodes" / "ExampleNode"
            write_file(hand, "LICENSE", MIT_LICENSE)
            write_file(hand, "__init__.py", b"ok\n")
            write_file(hand, ".gitkeep", b"")
            bogus = "0123456789abcdef0123456789abcdef01234567"
            with self.assertRaisesRegex(pack.PackError, "hand-installed or plain directory"):
                pack.pack_archive(**pack_kwargs(hand, bogus, base / "out"))

    def test_refuses_dirty_worktree_and_does_not_write(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n"}, dirty={"extra.py": b"uncommitted\n"})
            output = base / "out"
            with self.assertRaisesRegex(pack.PackError, "dirty"):
                pack.pack_archive(**pack_kwargs(repo, commit, output))
            self.assertFalse(any(output.rglob("*.zip")))

    def test_refuses_commit_not_in_repository(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            init_repo(repo, {"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n"})
            missing = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            with self.assertRaisesRegex(pack.PackError, "does not contain the declared commit"):
                pack.pack_archive(**pack_kwargs(repo, missing, base / "out"))

    def test_excludes_pycache_and_keeps_source(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {
                "LICENSE": MIT_LICENSE,
                "__init__.py": b"ok\n",
                "pkg/__init__.py": b"x\n",
                "pkg/__pycache__/mod.cpython-313.pyc": b"cache",
            })
            output = base / "out"
            result = pack.pack_archive(**pack_kwargs(repo, commit, output))
            with zipfile.ZipFile(output / "ExampleNode" / f"{commit}.zip") as archive:
                names = set(archive.namelist())
            self.assertIn("ExampleNode/pkg/__init__.py", names)
            self.assertTrue(all("__pycache__" not in name for name in names))
            self.assertEqual(result["excluded"], [{"path": "pkg/__pycache__/mod.cpython-313.pyc", "reason": "cache"}])

    def test_refuses_models_secrets_and_venv_paths(self):
        cases = {
            "model": ({"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n", "weights/model.safetensors": b"w"}, "model weights"),
            "secret": ({"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n", ".env": b"TOKEN=1\n"}, "secret or credential"),
            "venv": ({"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n", ".venv/lib/site.py": b"x"}, "virtualenv"),
        }
        for label, (files, pattern) in cases.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as temp:
                base = Path(temp)
                repo = base / "repo"
                commit = init_repo(repo, files)
                with self.assertRaisesRegex(pack.PackError, pattern):
                    pack.pack_archive(**pack_kwargs(repo, commit, base / "out"))

    def test_refuses_missing_or_non_redistributable_license(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {"__init__.py": b"ok\n", "README.md": b"no license\n"})
            with self.assertRaisesRegex(pack.PackError, "no license material"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out"))
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {"LICENSE": ARR_LICENSE, "__init__.py": b"ok\n"})
            with self.assertRaisesRegex(pack.PackError, "does not permit redistribution"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out"))

    def test_path_validators_reject_traversal_ads_device_names_and_case_conflicts(self):
        with self.assertRaisesRegex(pack.PackError, "Unsafe"):
            pack.posix_relative_path("../secret", kind="tree path")
        with self.assertRaisesRegex(pack.PackError, "Unsafe|Invalid"):
            pack.posix_relative_path("/abs/root", kind="tree path")
        with self.assertRaisesRegex(pack.PackError, "Invalid"):
            pack.posix_relative_path(r"foo\\bar", kind="tree path")
        with self.assertRaisesRegex(pack.PackError, "Invalid"):
            pack.posix_relative_path("foo:secret", kind="tree path")
        with self.assertRaisesRegex(pack.PackError, "Reserved device name"):
            pack.posix_relative_path("CON.py", kind="tree path")
        with self.assertRaisesRegex(pack.PackError, "Unsafe"):
            pack.posix_relative_path("dir.", kind="tree path")
        with self.assertRaisesRegex(pack.PackError, "case-conflicting"):
            pack.select_source_files([
                ("README.md", "a" * 40, b"A"),
                ("readme.md", "b" * 40, b"B"),
            ])

    def test_refuses_dependencies_without_wheel_lock_or_python_tags(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {
                "LICENSE": MIT_LICENSE,
                "__init__.py": b"ok\n",
                "requirements.txt": b"example-dep==1.0.0\n",
            })
            with self.assertRaisesRegex(pack.PackError, "wheel lock"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out"))

            lock_path = base / "bad-lock.json"
            lock_path.write_bytes(json.dumps({
                "schemaVersion": 1,
                "targets": [{
                    "pythonTag": "cp313",
                    "abiTag": "cp313",
                    "platformTag": "win_amd64",
                    "wheels": [{
                        "name": "example-dep",
                        "requires": "==1.0.0",
                        "version": "1.0.0",
                        "filename": "example_dep-1.0.0-py3-none-any.whl",
                        "bytes": 12,
                        "sha256": "ab" * 32,
                        "url": "https://files.pythonhosted.org/packages/example_dep-1.0.0-py3-none-any.whl?download=1",
                    }],
                }],
            }).encode())
            with self.assertRaisesRegex(pack.PackError, "query, fragment, auth or custom port"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out", wheel_lock_file=lock_path))

            empty_tags = base / "no-tags.json"
            empty_tags.write_bytes(json.dumps({"schemaVersion": 1, "targets": []}).encode())
            with self.assertRaisesRegex(pack.PackError, "Python/ABI/platform target"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out", wheel_lock_file=empty_tags))

    def test_wheel_lock_companion_is_covered_by_archive_digest(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            requirements = b"example-dep==1.0.0\n"
            commit = init_repo(repo, {
                "LICENSE": MIT_LICENSE,
                "__init__.py": b"ok\n",
                "requirements.txt": requirements,
            })
            lock_bytes = valid_wheel_lock()
            lock_path = base / "lock.json"
            lock_path.write_bytes(lock_bytes)
            output = base / "out"
            result = pack.pack_archive(**pack_kwargs(repo, commit, output, wheel_lock_file=lock_path))
            self.assertEqual(result["requirements"], {
                "path": "requirements.txt",
                "sha256": sha(requirements),
                "wheelLock": "bgx-wheel-lock.json",
            })
            zip_path = output / "ExampleNode" / f"{commit}.zip"
            with zipfile.ZipFile(zip_path) as archive:
                names = archive.namelist()
                self.assertIn("ExampleNode/bgx-wheel-lock.json", names)
                self.assertEqual(archive.read("ExampleNode/bgx-wheel-lock.json"), lock_bytes)
                self.assertEqual(archive.read("ExampleNode/requirements.txt"), requirements)
            self.assertEqual(result["archive"]["sha256"], sha(zip_path.read_bytes()))
            self.assertEqual(result["companions"][0]["path"], "bgx-wheel-lock.json")

    def test_refuses_wheel_lock_that_omits_declared_requirements(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {
                "LICENSE": MIT_LICENSE,
                "__init__.py": b"ok\n",
                "requirements.txt": b"example-dep==1.0.0\nother-dep==2.0.0\n",
            })
            lock_path = base / "lock.json"
            lock_path.write_bytes(valid_wheel_lock(["example-dep"]))
            with self.assertRaisesRegex(pack.PackError, "does not cover declared requirements"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out", wheel_lock_file=lock_path))

    def test_refuses_path_escape_in_wheel_lock_destination(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {
                "LICENSE": MIT_LICENSE,
                "__init__.py": b"ok\n",
                "requirements.txt": b"example-dep==1.0.0\n",
            })
            lock_path = base / "lock.json"
            lock_path.write_bytes(valid_wheel_lock())
            with self.assertRaisesRegex(pack.PackError, "Unsafe"):
                pack.pack_archive(**pack_kwargs(
                    repo, commit, base / "out",
                    wheel_lock_file=lock_path,
                    wheel_lock_path="../outside.json",
                ))

    def test_strip_components_zero_has_no_wrapper_root(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n"})
            output = base / "out"
            pack.pack_archive(**pack_kwargs(repo, commit, output, strip_components=0))
            with zipfile.ZipFile(output / "ExampleNode" / f"{commit}.zip") as archive:
                self.assertEqual(set(archive.namelist()), {"LICENSE", "__init__.py"})

    def test_existing_different_archive_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n"})
            output = base / "out" / "ExampleNode"
            output.mkdir(parents=True)
            target = output / f"{commit}.zip"
            target.write_bytes(b"not-the-archive")
            with self.assertRaisesRegex(pack.PackError, "refusing to overwrite"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out"))
            self.assertEqual(target.read_bytes(), b"not-the-archive")

    def test_rejects_unsafe_folder_name_and_node_id(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n"})
            with self.assertRaisesRegex(pack.PackError, "folderName"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out", folder_name="bad/name"))
            with self.assertRaisesRegex(pack.PackError, "Reserved device name"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out", folder_name="CON"))
            with self.assertRaisesRegex(pack.PackError, "reverse-domain"):
                pack.pack_archive(**pack_kwargs(repo, commit, base / "out", node_id="NotValid"))

    def test_cli_writes_manifest_and_zip(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            repo = base / "repo"
            commit = init_repo(repo, {"LICENSE": MIT_LICENSE, "__init__.py": b"ok\n"})
            output = base / "out"
            code = pack.main([
                "--source-repo", str(repo),
                "--git-commit", commit,
                "--folder-name", "ExampleNode",
                "--id", "org.example.node",
                "--homepage", "https://github.com/example/ExampleNode",
                "--license", "MIT",
                "--output-dir", str(output),
            ])
            self.assertEqual(code, 0)
            self.assertTrue((output / "ExampleNode" / f"{commit}.zip").is_file())
            manifest = json.loads((output / "ExampleNode" / f"{commit}.manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["gitCommit"], commit)
            self.assertEqual(manifest["archive"]["sha256"], sha((output / "ExampleNode" / f"{commit}.zip").read_bytes()))


if __name__ == "__main__":
    unittest.main()
