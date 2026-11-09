"""Compile the frozen certified UI graph, including subgraphs and bypass edges.

No prompt is submitted. The object-info snapshot is the explicit compilation
dependency; node defaults come from the frozen source, not current UI defaults.
"""
from pathlib import Path
import hashlib
import json
import urllib.request
import argparse

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'workflows/source.json'
EXPECTED = '9b017cde7f0564968cc7d2d2d3d044741a991e6662674b3913d5d77f92e9af71'

def compile_graph(source, declarations, terminal=113):
    definitions = {g['id']: g for g in source['definitions']['subgraphs']}
    output, provenance, busy = {}, {}, set()

    def links(scope):
        return {v['id'] if isinstance(v, dict) else v[0]:
                (v['origin_id'], v['origin_slot']) if isinstance(v, dict) else (v[1], v[2])
                for v in scope['links']}

    def fields(node, declaration):
        values = iter(node.get('widgets_values', []))
        result = {}
        def dynamic_fields(prefix, selector, value):
            result[prefix] = value[selector]
            for child, nested in value.items():
                if child == selector:
                    continue
                if isinstance(nested, dict):
                    dynamic_fields(prefix + '.' + child, child, nested)
                else:
                    result[prefix + '.' + child] = nested
        def widget(name, spec):
            value = next(values, None)
            if value is None:
                return None
            if spec[0] == 'COMFY_DYNAMICCOMBO_V3':
                options = spec[1]['options']
                chosen = next((o for o in options if o['key'] == value), None)
                if chosen is None:
                    raise ValueError(f'unknown dynamic combo value {name}={value}')
                nested = {name: value}
                for field, child in chosen['inputs'].get('required', {}).items():
                    nested[field] = widget(field, child)
                return nested
            return value
        ordered = declaration.get('input_order', {})
        for group in ['required', 'optional']:
            specs = declaration['input'].get(group, {})
            for name in ordered.get(group, list(specs)):
                spec = specs[name]
                kind = spec[0]
                options = spec[1] if len(spec) > 1 and isinstance(spec[1], dict) else {}
                if options.get('hidden'):
                    continue
                if isinstance(kind, list) or kind in ['INT', 'FLOAT', 'STRING', 'BOOLEAN', 'COMBO', 'COMFY_DYNAMICCOMBO_V3']:
                    value = widget(name, spec)
                    if value is not None:
                        if kind == 'COMFY_DYNAMICCOMBO_V3':
                            # Prompt JSON uses dotted paths; the execution
                            # engine reconstructs the nested execute() argument.
                            dynamic_fields(name, name, value)
                        else:
                            result[name] = value
                    if options.get('control_after_generate'):
                        next(values, None)
        return result

    def resolve(scope, node_id, slot, prefix='', external=None):
        external = external or {}
        if node_id == -10:
            return external.get(slot)
        node = next(n for n in scope['nodes'] if n['id'] == node_id)
        kind = node['type']
        edge_map = links(scope)
        def incoming(port):
            link = port.get('link')
            return resolve(scope, *edge_map[link], prefix, external) if link is not None else None
        # The original UI disables the entire standard branch as one group.
        # Explicit terminal 168 selects that branch; its optional third
        # continuation (174) stays bypassed, retaining the original 3 chunks.
        selected_standard = terminal == 168 and prefix == '' and node_id >= 147 and node_id != 174
        if node.get('mode') == 4 and not selected_standard:
            target_type = node['outputs'][slot]['type']
            ports = [p for p in node['inputs'] if p['type'] == target_type and p.get('link') is not None]
            if len(ports) != 1:
                raise ValueError(f'ambiguous bypass {prefix}{node_id}')
            return incoming(ports[0])
        if kind in ['PrimitiveInt', 'PrimitiveFloat']:
            return node['widgets_values'][0]
        if kind in definitions:
            graph = definitions[kind]
            widgets = iter(node.get('widgets_values', []))
            supplied = {}
            for index, port in enumerate(graph['inputs']):
                default = next(widgets, None) if port['type'] in ['INT', 'FLOAT', 'STRING', 'BOOLEAN', 'COMBO'] else None
                outer = next((p for p in node['inputs'] if p['name'] == port['name']), {})
                supplied[index] = incoming(outer) if outer.get('link') is not None else default
            exits = [v for v in graph['links'] if v['target_id'] == -20 and v['target_slot'] == slot]
            if len(exits) != 1:
                raise ValueError('subgraph requires one source per output')
            exit = exits[0]
            return resolve(graph, exit['origin_id'], exit['origin_slot'], f'{prefix}{node_id}:', supplied)
        key = f'{prefix}{node_id}'
        if key in output:
            return [key, slot]
        if key in busy:
            raise ValueError('cycle in certified graph')
        busy.add(key)
        declaration = declarations[kind]
        inputs = fields(node, declaration)
        for port in node.get('inputs', []):
            value = incoming(port)
            if value is not None:
                inputs[port['name']] = value
        required = declaration['input'].get('required', {})
        missing = set(required) - set(inputs)
        if missing:
            raise ValueError(f'{key} {kind}: missing {sorted(missing)}')
        output[key] = {'class_type': kind, 'inputs': inputs}
        provenance[key] = {'scope': prefix, 'sourceNodeId': node_id, 'classType': kind}
        busy.remove(key)
        return [key, slot]

    resolve(source, terminal, 0)
    return output, provenance

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--terminal', type=int, choices=[113,168], default=168)
    args = parser.parse_args()
    raw = SOURCE.read_bytes()
    if hashlib.sha256(raw).hexdigest() != EXPECTED:
        raise ValueError('frozen certified source changed')
    source = json.loads(raw)
    snapshot = ROOT / 'workflows/object-info.json'
    if not snapshot.exists():
        kinds = {n['type'] for n in source['nodes']}
        for graph in source['definitions']['subgraphs']:
            kinds.update(n['type'] for n in graph['nodes'])
        kinds -= {g['id'] for g in source['definitions']['subgraphs']}
        kinds -= {'Note', 'MarkdownNote', 'PrimitiveInt', 'PrimitiveFloat'}
        declarations = {}
        for kind in sorted(kinds):
            with urllib.request.urlopen('http://127.0.0.1:8188/object_info/' + kind, timeout=10) as response:
                info = json.load(response)
            if kind not in info:
                raise ValueError(f'node is unavailable: {kind}')
            declarations[kind] = info[kind]
        snapshot.write_text(json.dumps(declarations, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
    graph, mapping = compile_graph(source, json.loads(snapshot.read_text(encoding='utf-8')), args.terminal)
    for filename, value in [('api.json', graph), ('node-map.json', mapping)]:
        (ROOT / 'workflows' / filename).write_text(json.dumps(value, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
    print(f'Compiled terminal {args.terminal}: {len(graph)} reachable native nodes; no generation submitted')

if __name__ == '__main__':
    main()
