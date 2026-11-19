#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Convert ComfyUI UI-format workflows to API prompt graphs.

Uses the running ComfyUI /object_info contract so widget names and types
match the installed node set. This is a headless equivalent of the editor
'Export (API)' action (the server has no export route). Does not submit
prompts or start generation.
"""
from __future__ import annotations

import hashlib
import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COMFY = "http://127.0.0.1:8188"
SKIP_CLASSES = {"MarkdownNote", "Note", "Reroute"}
INPUT_NODE = -10
OUTPUT_NODE = -20


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def dump_api(obj) -> bytes:
    return (json.dumps(obj, ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8")


def fetch_object_info() -> dict:
    cache = Path(__file__).resolve().parents[4] / "artifacts" / "cache" / "ltx25-sources" / "object_info.json"
    # parents[4] may be wrong; prefer repo-relative
    repo = next(parent for parent in ROOT.parents if (parent / 'AGENTS.md').exists())
    cache = repo / "artifacts" / "cache" / "ltx25-sources" / "object_info.json"
    cache.parent.mkdir(parents=True, exist_ok=True)
    url = COMFY + "/object_info"
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read()
    cache.write_bytes(data)
    info = json.loads(data.decode("utf-8"))
    print(f"object_info classes={len(info)} cache={cache}", flush=True)
    return info


def link_items(links):
    items = []
    if not links:
        return items
    if isinstance(links, dict):
        links = list(links.values())
    for item in links:
        if isinstance(item, dict):
            items.append({
                "id": item.get("id"),
                "origin_id": item.get("origin_id"),
                "origin_slot": item.get("origin_slot"),
                "target_id": item.get("target_id"),
                "target_slot": item.get("target_slot"),
            })
        elif isinstance(item, (list, tuple)) and len(item) >= 6:
            items.append({
                "id": item[0],
                "origin_id": item[1],
                "origin_slot": item[2],
                "target_id": item[3],
                "target_slot": item[4],
            })
    return items


def index_links(links):
    by_id = {}
    by_target = {}
    for item in link_items(links):
        if item["id"] is not None:
            by_id[item["id"]] = item
        by_target[(item["target_id"], item["target_slot"])] = item
    return by_id, by_target


def subgraph_map(definitions):
    if not definitions:
        return {}
    sgs = definitions.get("subgraphs") if isinstance(definitions, dict) else None
    if sgs is None:
        return {}
    if isinstance(sgs, dict):
        return {key: value for key, value in sgs.items()}
    return {item["id"]: item for item in sgs}


def class_inputs(info: dict, class_type: str) -> dict:
    spec = info.get(class_type) or {}
    inputs = {}
    block = spec.get("input") or {}
    for group in ("required", "optional"):
        fields = block.get(group) or {}
        if isinstance(fields, dict):
            inputs.update(fields)
    return inputs


def widget_names(node: dict) -> list[str]:
    names = []
    for inp in node.get("inputs") or []:
        widget = inp.get("widget")
        if widget:
            names.append(widget.get("name") or inp.get("name"))
    return names


def node_title(node: dict) -> str:
    return node.get("title") or (node.get("properties") or {}).get("Node name for S&R") or node.get("type") or ""


def prefix_id(prefix: str, node_id) -> str:
    return f"{prefix}{node_id}"


def is_virtual(node_id) -> bool:
    try:
        return int(node_id) in (INPUT_NODE, OUTPUT_NODE)
    except (TypeError, ValueError):
        return node_id in (INPUT_NODE, OUTPUT_NODE, str(INPUT_NODE), str(OUTPUT_NODE))


def is_reroute(node) -> bool:
    return bool(node) and node.get("type") == "Reroute"


def follow_reroute(origin_id, origin_slot, nodes_by_id, by_id, seen=None):
    seen = set() if seen is None else set(seen)
    node = nodes_by_id.get(origin_id)
    if not is_reroute(node):
        return origin_id, origin_slot
    if origin_id in seen:
        raise RuntimeError(f"reroute cycle at {origin_id}")
    seen.add(origin_id)
    inp = (node.get("inputs") or [None])[0]
    link_id = None
    if isinstance(inp, dict):
        if inp.get("link") is not None:
            link_id = inp["link"]
        else:
            links = inp.get("linkIds") or inp.get("links")
            if isinstance(links, list) and links:
                link_id = links[0]
    link = by_id.get(link_id) if link_id is not None else None
    if link is None:
        raise RuntimeError(f"reroute {origin_id} missing input link")
    return follow_reroute(link["origin_id"], link["origin_slot"], nodes_by_id, by_id, seen)


class Converter:
    def __init__(self, info: dict, definitions: dict):
        self.info = info
        self.subgraphs = subgraph_map(definitions)
        self.api: dict[str, dict] = {}

    def convert(self, graph: dict, prefix: str = "") -> dict:
        self._emit_graph(graph, prefix)
        self._rewrite_instance_outputs()
        return self.api

    def _rewrite_instance_outputs(self):
        # Subgraph instances earlier in the node list may be expanded before
        # sibling producers exist. Rewrite leftover [instance, slot] inputs
        # after every instance output is cached.
        mapping = {}
        for (prefix, local_id, slot), ref in getattr(self, "_out_cache", {}).items():
            mapping[(prefix_id(prefix, local_id), slot)] = ref
        for node in self.api.values():
            inputs = node.get("inputs") or {}
            for key, value in list(inputs.items()):
                if not (isinstance(value, list) and len(value) >= 2):
                    continue
                ref = mapping.get((str(value[0]), value[1]))
                if ref is None:
                    continue
                if isinstance(ref, tuple) and ref and ref[0] == "__value__":
                    inputs[key] = ref[1]
                else:
                    inputs[key] = [ref[0], ref[1]]

    def _emit_graph(self, graph: dict, prefix: str) -> dict:
        """Return map (local_id, output_slot) -> (api_id, slot) for this graph."""
        outputs = {}
        by_id, by_target = index_links(graph.get("links"))
        nodes = {n["id"]: n for n in graph.get("nodes") or []}

        # Emit subgraphs first so their outputs can be referenced.
        instance_outputs = {}
        for node in graph.get("nodes") or []:
            typ = node.get("type")
            if typ in self.subgraphs:
                inst_prefix = prefix_id(prefix, node["id"]) + ":"
                instance_outputs[node["id"]] = self._emit_subgraph(
                    node, self.subgraphs[typ], inst_prefix, prefix, by_id, by_target
                )
                self.remember_outputs(prefix, {(node["id"], slot): ref for slot, ref in instance_outputs[node["id"]].items()})

        for node in graph.get("nodes") or []:
            typ = node.get("type")
            if typ in self.subgraphs:
                for slot, ref in instance_outputs[node["id"]].items():
                    outputs[(node["id"], slot)] = ref
                continue
            if typ in SKIP_CLASSES:
                continue
            if typ not in self.info and typ not in self.subgraphs:
                # Unknown concrete class: keep it so freeze assertions can fail loudly.
                pass
            api_id = prefix_id(prefix, node["id"])
            if typ not in SKIP_CLASSES:
                self.api[api_id] = self._build_node(node, api_id, prefix, by_id, by_target, instance_outputs, typ, nodes_by_id=nodes)
                # each output slot maps to itself
                for slot, _out in enumerate(node.get("outputs") or []):
                    outputs[(node["id"], slot)] = (api_id, slot)
        self.remember_outputs(prefix, outputs)
        return outputs

    def _instance_widget_values(self, instance: dict) -> dict:
        names = widget_names(instance)
        values = instance.get("widgets_values") or []
        named = instance.get("widgets_values_named")
        if isinstance(named, dict) and named:
            return dict(named)
        out = {}
        for name, value in zip(names, values):
            out[name] = value
        return out

    def _input_link_id(self, inp) -> int | None:
        if not isinstance(inp, dict):
            return None
        if inp.get("link") is not None:
            return inp["link"]
        links = inp.get("linkIds") or inp.get("links")
        if isinstance(links, list) and links:
            return links[0]
        return None

    def _emit_subgraph(self, instance, subgraph, inst_prefix, parent_prefix, parent_by_id, parent_by_target):
        # Bind each subgraph input slot to a parent source or a widget value.
        bindings = {}  # slot index -> ("link", api_id, slot) or ("value", value)
        widget_values = self._instance_widget_values(instance)
        inst_inputs = instance.get("inputs") or []
        sg_inputs = subgraph.get("inputs") or []
        for slot, sg_inp in enumerate(sg_inputs):
            inst_inp = inst_inputs[slot] if slot < len(inst_inputs) else None
            link_id = self._input_link_id(inst_inp) if inst_inp else None
            if link_id is not None and link_id in parent_by_id:
                link = parent_by_id[link_id]
                bindings[slot] = ("pending_parent", link["origin_id"], link["origin_slot"])
            else:
                name = (inst_inp or {}).get("name") or sg_inp.get("name")
                if name in widget_values:
                    bindings[slot] = ("value", widget_values[name])
                else:
                    bindings[slot] = ("value", None)

        # Resolve pending parent links after we have parent outputs:
        # parent origins may be sibling subgraphs; those are already emitted
        # if we process instances in dependency order. We'll resolve in _resolve_parent.
        self._parent_ctx = {
            "prefix": parent_prefix,
            "by_id": parent_by_id,
        }

        inner_links = index_links(subgraph.get("links"))
        # Map output slot -> inner origin
        output_producers = {}
        for item in link_items(subgraph.get("links")):
            if is_virtual(item["target_id"]) and int(item["target_id"]) == OUTPUT_NODE:
                output_producers[item["target_slot"]] = (item["origin_id"], item["origin_slot"])

        # Recursively emit inner graph nodes (inner subgraphs if any)
        # Build a synthetic graph for inner nodes so we can reuse _emit_graph? 
        # Inner nodes already listed; they may include nested subgraph types.
        inner_graph = {
            "nodes": [n for n in (subgraph.get("nodes") or []) if not is_virtual(n["id"])],
            "links": subgraph.get("links"),
        }
        # We need custom input binding for links from INPUT_NODE.
        inner_outputs = self._emit_inner(inner_graph, inst_prefix, bindings, inner_links, parent_prefix)

        slot_map = {}
        inner_nodes = {n["id"]: n for n in (subgraph.get("nodes") or [])}
        inner_by_id, _inner_by_target = inner_links
        for slot, (origin_id, origin_slot) in output_producers.items():
            origin_id, origin_slot = follow_reroute(origin_id, origin_slot, inner_nodes, inner_by_id)
            if is_virtual(origin_id) and int(origin_id) == INPUT_NODE:
                binding = bindings.get(origin_slot)
                if binding is None:
                    raise RuntimeError("subgraph %s output %s missing input binding %s" % (instance["id"], slot, origin_slot))
                if binding[0] == "value":
                    slot_map[slot] = ("__value__", binding[1])
                    continue
                if binding[0] == "pending_parent":
                    _, parent_origin, parent_slot = binding
                    cache = getattr(self, "_out_cache", {})
                    key = (parent_prefix, parent_origin, parent_slot)
                    if key in cache:
                        slot_map[slot] = cache[key]
                    else:
                        slot_map[slot] = (prefix_id(parent_prefix, parent_origin), parent_slot)
                    continue
                raise RuntimeError("subgraph %s output %s bad binding %s" % (instance["id"], slot, binding[0]))
            ref = inner_outputs.get((origin_id, origin_slot))
            if ref is None:
                raise RuntimeError("subgraph %s output %s missing producer %s:%s" % (instance["id"], slot, origin_id, origin_slot))
            slot_map[slot] = ref
        return slot_map

    def _emit_inner(self, graph, prefix, bindings, inner_links, parent_prefix):
        by_id, by_target = inner_links
        outputs = {}
        instance_outputs = {}
        for node in graph.get("nodes") or []:
            typ = node.get("type")
            if typ in self.subgraphs:
                inst_prefix = prefix_id(prefix, node["id"]) + ":"
                instance_outputs[node["id"]] = self._emit_subgraph(
                    node, self.subgraphs[typ], inst_prefix, prefix, by_id, by_target
                )
                self.remember_outputs(prefix, {(node["id"], slot): ref for slot, ref in instance_outputs[node["id"]].items()})
                for slot, ref in instance_outputs[node["id"]].items():
                    outputs[(node["id"], slot)] = ref
        for node in graph.get("nodes") or []:
            typ = node.get("type")
            if typ in self.subgraphs or typ in SKIP_CLASSES:
                continue
            api_id = prefix_id(prefix, node["id"])
            self.api[api_id] = self._build_node(
                node, api_id, prefix, by_id, by_target, instance_outputs, typ,
                input_bindings=bindings, parent_prefix=parent_prefix,
                nodes_by_id={n["id"]: n for n in graph.get("nodes") or []},
            )
            for slot, _out in enumerate(node.get("outputs") or []):
                outputs[(node["id"], slot)] = (api_id, slot)
        self.remember_outputs(prefix, outputs)
        return outputs

    def _resolve_origin(self, origin_id, origin_slot, prefix, instance_outputs, input_bindings=None, parent_prefix="", nodes_by_id=None, by_id=None):
        if nodes_by_id and by_id:
            origin_id, origin_slot = follow_reroute(origin_id, origin_slot, nodes_by_id, by_id)
        if is_virtual(origin_id) and int(origin_id) == INPUT_NODE:
            binding = (input_bindings or {}).get(origin_slot)
            if binding is None:
                raise RuntimeError(f"missing subgraph input binding slot {origin_slot}")
            if binding[0] == "value":
                return ("value", binding[1])
            # pending_parent: origin in parent graph
            _, parent_origin, parent_slot = binding
            # If parent origin is a subgraph instance, use its mapped output
            # We stored instance_outputs only for the current graph. For parent,
            # look up already-emitted api ids: parent prefix + id, or instance map
            # is not available here. Use already-computed api graph: parent nodes
            # of subgraph type were emitted; we need their output map.
            # Convention: look for api node parent_prefix+origin, else instance map
            # stored on self._instance_out_cache
            cache = getattr(self, "_out_cache", {})
            key = (parent_prefix, parent_origin, parent_slot)
            if key in cache:
                ref = cache[key]
                if isinstance(ref, tuple) and ref and ref[0] == "__value__":
                    return ("value", ref[1])
                return ("link",) + ref
            # Concrete parent node may not have been emitted yet; its API id is deterministic.
            # Subgraph-instance parents must already be in the cache (emitted first).
            return ("link", prefix_id(parent_prefix, parent_origin), parent_slot)
        if origin_id in instance_outputs:
            ref = instance_outputs[origin_id].get(origin_slot)
            if ref is None:
                raise RuntimeError(f"instance {origin_id} missing output {origin_slot}")
            if isinstance(ref, tuple) and ref and ref[0] == "__value__":
                return ("value", ref[1])
            return ("link",) + ref
        cache = getattr(self, "_out_cache", {})
        key = (prefix, origin_id, origin_slot)
        if key in cache:
            ref = cache[key]
            if isinstance(ref, tuple) and ref and ref[0] == "__value__":
                return ("value", ref[1])
            return ("link",) + ref
        return ("link", prefix_id(prefix, origin_id), origin_slot)

    def _build_node(self, node, api_id, prefix, by_id, by_target, instance_outputs, typ,
                    input_bindings=None, parent_prefix="", nodes_by_id=None):
        inputs = {}
        # Linked inputs
        for slot, inp in enumerate(node.get("inputs") or []):
            name = inp.get("name")
            link_id = self._input_link_id(inp)
            if link_id is None:
                continue
            link = by_id.get(link_id)
            if link is None:
                continue
            resolved = self._resolve_origin(
                link["origin_id"], link["origin_slot"], prefix, instance_outputs,
                input_bindings=input_bindings, parent_prefix=parent_prefix,
                nodes_by_id=nodes_by_id, by_id=by_id,
            )
            if resolved[0] == "value":
                if resolved[1] is not None:
                    inputs[name] = resolved[1]
            else:
                inputs[name] = [resolved[1], resolved[2]]

        # Widget values for unlinked inputs
        named = node.get("widgets_values_named")
        specs = class_inputs(self.info, typ)
        if isinstance(named, dict) and named:
            for name, value in named.items():
                if name in inputs:
                    continue
                if specs and name not in specs:
                    continue
                inputs[name] = value
        else:
            values = node.get("widgets_values") or []
            names = widget_names(node)
            for name, value in zip(names, values):
                if name in inputs:
                    continue
                if specs and name not in specs:
                    continue
                inputs[name] = value

        built = {"class_type": typ, "inputs": inputs}
        title = node_title(node)
        if title:
            built["_meta"] = {"title": title}
        return built

    def remember_outputs(self, prefix, outputs):
        cache = getattr(self, "_out_cache", {})
        for (local_id, slot), ref in outputs.items():
            cache[(prefix, local_id, slot)] = ref
        self._out_cache = cache


def convert_workflow(workflow: dict, info: dict) -> dict:
    definitions = workflow.get("definitions") or (workflow.get("extra") or {}).get("definitions") or {}
    conv = Converter(info, definitions)
    # Emit in two phases: we call convert which emits the top graph.
    # To resolve parent subgraph origins reliably, emit all top-level subgraphs first
    # by using convert() which already emits subgraphs first.
    api = conv.convert(workflow)
    # Drop leftover empty/note if any slipped in
    return {key: value for key, value in api.items() if value.get("class_type") not in SKIP_CLASSES}


def main() -> int:
    sources = {
        "t2v": Path(r"L:\00Dev\bgxiong-ComfyUI\API\LTX2.5\video_ltx2_5_t2v.json"),
        "i2v": Path(r"L:\00Dev\bgxiong-ComfyUI\API\LTX2.5\video_ltx2_5_i2v.json"),
        "a2v": Path(r"L:\00Dev\bgxiong-third-party-adapters\artifacts\cache\ltx25-sources\LTX-2.5_A2V_Two_Stage_Distilled.json"),
    }
    info = fetch_object_info()
    export_dir = ROOT / "workflows" / "export"
    export_dir.mkdir(parents=True, exist_ok=True)
    versions = {}
    try:
        with urllib.request.urlopen(COMFY + "/system_stats", timeout=15) as response:
            versions = json.loads(response.read().decode("utf-8"))
    except Exception as exc:
        print(f"system_stats warning: {exc}", flush=True)
    for name, path in sources.items():
        if not path.is_file():
            print(f"missing source {path}", file=sys.stderr)
            return 1
        raw = path.read_bytes()
        workflow = json.loads(raw.decode("utf-8-sig"))
        api = convert_workflow(workflow, info)
        out = export_dir / f"{name}.export.json"
        data = dump_api(api)
        out.write_bytes(data)
        print(f"{name}: nodes={len(api)} bytes={len(data)} sha256={sha256(data)} from={path}", flush=True)
        # sanity: required ids
        if name == "t2v":
            for key in ("452", "450", "449", "451", "448", "409", "454", "429", "421", "75"):
                if key not in api:
                    print(f"T2V missing node {key}", file=sys.stderr)
                    return 1
                print(f"  {key} {api[key].get('class_type')}", flush=True)
        if name == "i2v":
            for key in ("409", "413", "425", "418", "421", "395", "403", "407", "414", "423", "442", "75"):
                if key not in api:
                    print(f"I2V missing node {key}", file=sys.stderr)
                    return 1
                print(f"  {key} {api[key].get('class_type')}", flush=True)
        if name == "a2v":
            for key in ("5508", "5512", "5511", "5601", "2004", "5600", "4852", "5014:5556", "5014:5506", "5014:1241", "5514:3059"):
                present = key in api
                print(f"  {key} {'OK '+api[key].get('class_type') if present else 'MISSING'}", flush=True)
            if "5014:5556" in api:
                print("  5014:5556.inputs", api["5014:5556"]["inputs"], flush=True)
            if "5514:3059" in api:
                print("  5514:3059.inputs", api["5514:3059"]["inputs"], flush=True)
    stats_path = ROOT / "workflows" / "export" / "_export_env.json"
    stats_path.write_text(json.dumps(versions, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
