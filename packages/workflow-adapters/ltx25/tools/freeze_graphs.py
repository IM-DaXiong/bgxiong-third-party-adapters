#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""从 workflows/export 确定性生成冻结 API 图，并可 --check 逐字节复核。"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import sys
from datetime import datetime, timezone, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXPORT_DIR = ROOT / "workflows" / "export"
FROZEN_DIR = ROOT / "workflows"
REMOVED_CLASSES = {"GemmaAPITextEncode", "StringContains", "ResolutionSelector"}
SHANGHAI = timezone(timedelta(hours=8), name="Asia/Shanghai")

EXPECTED_CLASSES = {
    "t2v": {
        "452": "PrimitiveStringMultiline",
        "450": "PrimitiveInt",
        "449": "PrimitiveInt",
        "451": "PrimitiveInt",
        "448": "PrimitiveInt",
        "75": "SaveVideo",
        "409": "ResolutionSelector",
        "454": "PrimitiveBoolean",
        "429": "RandomNoise",
        "421": "RandomNoise",
    },
    "i2v": {
        "409": None,
        "413": "PrimitiveInt",
        "425": None,
        "418": "PrimitiveInt",
        "421": "PrimitiveInt",
        "395": "LoadImage",
        "75": "SaveVideo",
        "403": "ResolutionSelector",
        "407": "PrimitiveBoolean",
        "414": "PrimitiveBoolean",
        "423": "RandomNoise",
        "442": "RandomNoise",
    },
    "a2v": {
        "5508": None,
        "5512": "PrimitiveFloat",
        "5511": "PrimitiveFloat",
        "5601": "PrimitiveFloat",
        "5514:3059": "EmptyLTXVLatentVideo",
        "5014:5506": "PrimitiveBoolean",
        "2004": "LoadImage",
        "5600": "LoadAudio",
        "4852": "SaveVideo",
        "5014:5556": "ComfySwitchNode",
        "5014:1241": "LTXVConditioning",
        "5014:5504": "GemmaAPITextEncode",
        "5014:5505": "GemmaAPITextEncode",
        "5014:5557": "StringContains",
        "5014:5558": "ComfySwitchNode",
        "5014:5560": "ComfySwitchNode",
        "5516:4832": "RandomNoise",
        "5517:4967": "RandomNoise",
    },
}

SOURCE_META = {
    "t2v": {
        "origin": "L:/00Dev/bgxiong-ComfyUI/API/LTX2.5/video_ltx2_5_t2v.json",
        "sha256": "f1a3395e6e4a0ce4ae762f3b5bfa78e1a4b4f39031bfd54c6ef3098cbc555790",
        "bytes": 84434,
    },
    "i2v": {
        "origin": "L:/00Dev/bgxiong-ComfyUI/API/LTX2.5/video_ltx2_5_i2v.json",
        "sha256": "04aaf0aeb7f3b65aff67641522caf5d737f610acb67ec19f27085e5a87c06359",
        "bytes": 94976,
    },
    "a2v": {
        "origin": "https://raw.githubusercontent.com/Lightricks/ComfyUI-LTXVideo/master/example_workflows/2.5/LTX-2.5_A2V_Two_Stage_Distilled.json",
        "sha256": "bcabfb983a6c4e5b4dfc9f4789f4522d8aa64076e34cb5b58791553389af252f",
        "bytes": 225460,
    },
}

PATCH_MAP = {
    "seeds": {
        "t2v": [{"node": "429", "field": "noise_seed", "offset": 0}, {"node": "421", "field": "noise_seed", "offset": 1}],
        "i2v": [{"node": "423", "field": "noise_seed", "offset": 0}, {"node": "442", "field": "noise_seed", "offset": 1}],
        "a2v": [{"node": "5516:4832", "field": "noise_seed", "offset": 0}, {"node": "5517:4967", "field": "noise_seed", "offset": 0}],
    },
    "fields": {
        "t2v": {"prompt": "452.value", "duration": "450.value", "fps": "449.value", "width": "451.value", "height": "448.value", "save": "75"},
        "i2v": {"prompt": "409.value", "duration": "413.value", "fps": "425.value", "width": "418.value", "height": "421.value", "image": "395.image", "save": "75"},
        "a2v": {"prompt": "5508.value", "duration": "5512.value", "fps": "5511.value", "audioStart": "5601.value", "width": "5514:3059.width", "height": "5514:3059.height", "useImage": "5014:5506.value", "image": "2004.image", "audio": "5600.audio", "save": "4852"},
    },
}

class FreezeError(SystemExit):
    pass

def fail(transform, node, expected, actual):
    print("FREEZE ASSERT FAILED %s node=%s expected=%r actual=%r" % (transform, node, expected, actual), file=sys.stderr)
    raise FreezeError(1)

def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()

def load_export(name):
    path = EXPORT_DIR / ("%s.export.json" % name)
    if not path.is_file():
        fail("load", name, str(path), "missing")
    return json.loads(path.read_text(encoding="utf-8"))

def dump_frozen(obj):
    return (json.dumps(obj, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")

def class_of(graph, node_id):
    node = graph.get(node_id)
    if not node:
        return None
    return node.get("class_type")

def refs_to(graph, target):
    hits = []
    for node_id, node in graph.items():
        for field, value in (node.get("inputs") or {}).items():
            if isinstance(value, list) and len(value) >= 2 and str(value[0]) == target:
                hits.append((node_id, field, value))
    return hits

def assert_classes(name, graph):
    for node_id, want in EXPECTED_CLASSES[name].items():
        actual = class_of(graph, node_id)
        if actual is None:
            fail("5.4", node_id, want or "present", "missing")
        if want is not None and actual != want:
            fail("5.4", node_id, want, actual)

def rewrite_refs(graph, old, new):
    count = 0
    for node in graph.values():
        inputs = node.get("inputs") or {}
        for field, value in list(inputs.items()):
            if value == old:
                inputs[field] = list(new)
                count += 1
    return count

def delete_nodes(graph, node_ids, transform):
    for node_id in node_ids:
        if node_id not in graph:
            fail(transform, node_id, "present", "missing")
        del graph[node_id]
    removed = set(node_ids)
    dangling = []
    for node_id, node in graph.items():
        for field, value in (node.get("inputs") or {}).items():
            if isinstance(value, list) and len(value) >= 2 and str(value[0]) in removed:
                dangling.append((node_id, field, value))
    if dangling:
        fail(transform, "dangling", "none", dangling)

def ft_t1(graph):
    if class_of(graph, "409") != "ResolutionSelector":
        fail("FT-T1", "409", "ResolutionSelector", class_of(graph, "409"))
    hits = refs_to(graph, "409")
    expected = {("451", "value", 0), ("448", "value", 1)}
    actual = {(n, f, v[1]) for n, f, v in hits}
    if actual != expected:
        fail("FT-T1", "refs-409", sorted(expected), sorted(actual))
    graph["451"]["inputs"]["value"] = 1280
    graph["448"]["inputs"]["value"] = 704
    delete_nodes(graph, ["409"], "FT-T1")

def ft_i1(graph):
    if class_of(graph, "403") != "ResolutionSelector":
        fail("FT-I1", "403", "ResolutionSelector", class_of(graph, "403"))
    hits = refs_to(graph, "403")
    expected = {("418", "value", 0), ("421", "value", 1)}
    actual = {(n, f, v[1]) for n, f, v in hits}
    if actual != expected:
        fail("FT-I1", "refs-403", sorted(expected), sorted(actual))
    graph["418"]["inputs"]["value"] = 1280
    graph["421"]["inputs"]["value"] = 704
    delete_nodes(graph, ["403"], "FT-I1")


def ft_a1(graph):
    node = graph.get("5014:5556")
    if not node or node.get("class_type") != "ComfySwitchNode":
        fail("FT-A1", "5014:5556", "ComfySwitchNode", class_of(graph, "5014:5556"))
    switch = (node.get("inputs") or {}).get("switch")
    if switch is not True:
        fail("FT-A1", "5014:5556.switch", True, switch)
    node["inputs"]["switch"] = False

def ft_a2(graph):
    for node_id, typ in (
        ("5014:5504", "GemmaAPITextEncode"),
        ("5014:5505", "GemmaAPITextEncode"),
        ("5014:5557", "StringContains"),
        ("5014:5558", "ComfySwitchNode"),
        ("5014:5560", "ComfySwitchNode"),
        ("5014:1241", "LTXVConditioning"),
    ):
        if class_of(graph, node_id) != typ:
            fail("FT-A2", node_id, typ, class_of(graph, node_id))
    sw_pos = graph["5014:5558"]["inputs"]
    sw_neg = graph["5014:5560"]["inputs"]
    if sw_pos.get("on_false") != ["5014:1241", 0]:
        fail("FT-A2", "5014:5558.on_false", ["5014:1241", 0], sw_pos.get("on_false"))
    if sw_neg.get("on_false") != ["5014:1241", 1]:
        fail("FT-A2", "5014:5560.on_false", ["5014:1241", 1], sw_neg.get("on_false"))
    removed = {"5014:5504", "5014:5505", "5014:5557", "5014:5558", "5014:5560"}
    for target in sorted(removed):
        for node_id, field, value in refs_to(graph, target):
            if node_id not in removed and target not in {"5014:5558", "5014:5560"}:
                fail("FT-A2", target + " extra consumer", "only deleted set", (node_id, field, value))
    rewrite_refs(graph, ["5014:5558", 0], ["5014:1241", 0])
    rewrite_refs(graph, ["5014:5560", 0], ["5014:1241", 1])
    delete_nodes(graph, ["5014:5504", "5014:5505", "5014:5557", "5014:5558", "5014:5560"], "FT-A2")

def ft_a3(graph):
    if class_of(graph, "2004") != "LoadImage":
        fail("FT-A3", "2004", "LoadImage", class_of(graph, "2004"))
    hits = refs_to(graph, "2004")
    if not hits:
        fail("FT-A3", "2004 refs", "at least output 0", hits)
    if any(value[1] != 0 for _, _, value in hits):
        fail("FT-A3", "2004 slots", "only output 0", hits)

def ft_all(graph, name):
    leftover = [node_id for node_id, node in graph.items() if node.get("class_type") in REMOVED_CLASSES]
    if leftover:
        fail("FT-ALL", name, "no removed classes", leftover)
    known = set(graph)
    dangling = []
    for node_id, node in graph.items():
        for field, value in (node.get("inputs") or {}).items():
            if isinstance(value, list) and len(value) >= 2 and str(value[0]) not in known:
                dangling.append((node_id, field, value))
    if dangling:
        fail("FT-ALL", name, "no dangling refs", dangling)

def freeze_one(name):
    graph = copy.deepcopy(load_export(name))
    assert_classes(name, graph)
    if name == "t2v":
        ft_t1(graph)
    elif name == "i2v":
        ft_i1(graph)
    elif name == "a2v":
        ft_a3(graph)
        ft_a1(graph)
        ft_a2(graph)
    ft_all(graph, name)
    return graph, dump_frozen(graph)

def write_source_json(frozen, exported_at):
    env_path = EXPORT_DIR / "_export_env.json"
    env = json.loads(env_path.read_text(encoding="utf-8")) if env_path.is_file() else {}
    system = env.get("system") or {}
    pkgs = {item["name"]: item.get("installed") for item in system.get("comfy_package_versions") or []}
    export_files = {}
    for name in ("t2v", "i2v", "a2v"):
        path = EXPORT_DIR / ("%s.export.json" % name)
        raw = path.read_bytes()
        export_files[name] = {"path": "workflows/export/%s.export.json" % name, "sha256": sha256_bytes(raw), "bytes": len(raw)}
    frozen_files = {}
    for name, data in frozen.items():
        frozen_files[name] = {"path": "workflows/%s.api.json" % name, "sha256": sha256_bytes(data), "bytes": len(data)}
    if not exported_at:
        exported_at = datetime.now(SHANGHAI).isoformat(timespec="seconds")
    source = {
        "schemaVersion": 1,
        "sources": SOURCE_META,
        "export": {
            "comfyVersion": system.get("comfyui_version"),
            "frontendVersion": pkgs.get("comfyui-frontend-package") or system.get("required_frontend_version"),
            "exportedAt": exported_at,
            "comfyRoot": "L:/AI/ComfyUI-Installs/ComfyUI (1)/ComfyUI",
            "endpoint": "http://127.0.0.1:8188",
            "method": "tools/export_graphs.py headless expander using live /object_info (no server Export(API) route)",
            "files": export_files,
        },
        "frozen": {"script": "tools/freeze_graphs.py", "files": frozen_files},
        "transforms": ["FT-T1", "FT-I1", "FT-A1", "FT-A2"],
        "deviations": [
            {"id": "FT-A1", "official": "Enhance positive prompt = true", "frozen": "false", "reason": "三模式统一关闭提示词扩写，不默认调用 LLM 改写用户提示词"},
            {"id": "FT-A2", "official": "Gemma API 分支（需 API Key 与自定义节点）", "frozen": "删除，固定本地编码", "reason": "无 API Key 时官方默认即走本地编码；避免自定义节点与 2.3 残留模型依赖"},
        ],
        "patchMap": PATCH_MAP,
    }
    path = ROOT / "SOURCE.json"
    data = (json.dumps(source, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    path.write_bytes(data)
    print("SOURCE.json bytes=%s sha256=%s" % (len(data), sha256_bytes(data)))

def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    frozen = {}
    for name in ("t2v", "i2v", "a2v"):
        graph, data = freeze_one(name)
        frozen[name] = data
        target = FROZEN_DIR / ("%s.api.json" % name)
        print("%s: nodes=%s bytes=%s sha256=%s -> %s" % (name, len(graph), len(data), sha256_bytes(data), target))
        if args.check:
            if not target.is_file():
                print("missing frozen file %s" % target, file=sys.stderr)
                return 1
            existing = target.read_bytes()
            if existing != data:
                print("CHECK FAILED %s: generated %s stored %s" % (name, sha256_bytes(data), sha256_bytes(existing)), file=sys.stderr)
                return 1
        else:
            FROZEN_DIR.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
    if not args.check:
        write_source_json(frozen, None)
    else:
        print("CHECK OK")
    return 0

if __name__ == "__main__":
    sys.exit(main())
