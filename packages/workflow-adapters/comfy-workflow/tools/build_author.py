#!/usr/bin/env python3
"""Build the standalone ComfyUI author project and recoverable workflow bundle."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tempfile
import zipfile


PACKAGE = Path(__file__).resolve().parents[1]
REPO = PACKAGE.parents[2]
CORE = REPO / 'crates/comfy-workflow-core'
CORE_DEPENDENCY = 'path = "../../../crates/comfy-workflow-core"'
STAGED_DEPENDENCY = 'path = "workflow-core"'
PROJECT_FILES = ('Cargo.toml', 'Cargo.lock', 'manifest.json', 'template-lock.json', 'README.md', 'workflow-templates.json')


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def read_json(path):
    return json.loads(path.read_bytes())


def stamp(data):
    return {'sha256': sha256(data), 'sizeBytes': len(data)}


def write_json(path, value):
    path.write_bytes((json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode())


def checked_path(root, relative):
    parts = PurePosixPath(relative).parts
    if not parts or any(part in ('.', '..', '/') or ':' in part or '\\' in part for part in parts):
        raise ValueError(f'Invalid resource path: {relative}')
    file = root.joinpath(*parts)
    if file.resolve() != file or not file.is_file():
        raise ValueError(f'Expected a regular file under {root}: {relative}')
    return file


def package_sources():
    for name in PROJECT_FILES:
        yield checked_path(PACKAGE, name), name
    for directory, suffix in (('src', '.rs'), ('schemas', '.json')):
        for file in sorted((PACKAGE / directory).rglob('*' + suffix)):
            relative = file.relative_to(PACKAGE).as_posix()
            yield checked_path(PACKAGE, relative), relative
    mirror = read_json(PACKAGE / 'template-lock.json')
    for row in mirror['generatedFiles']:
        name = row['target']
        if name == 'Cargo.lock':
            continue
        if not name.startswith('vendor-sdk/'):
            raise ValueError(f'Unexpected SDK template mirror target: {name}')
        file = checked_path(PACKAGE, name)
        if sha256(file.read_bytes()) != row['sha256']:
            raise ValueError(f'SDK mirror checksum changed: {name}')
        yield file, name
    yield checked_path(CORE, 'Cargo.toml'), 'workflow-core/Cargo.toml'
    for directory, suffix in (('src', '.rs'), ('contracts', '.json')):
        for file in sorted((CORE / directory).rglob('*' + suffix)):
            relative = file.relative_to(CORE).as_posix()
            yield checked_path(CORE, relative), 'workflow-core/' + relative
    yield checked_path(REPO, 'LICENSE'), 'LICENSE'


def staged_source(source, relative, project):
    original = source.read_bytes()
    target = project / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    content = original
    if relative == 'Cargo.toml':
        if original.count(CORE_DEPENDENCY.encode()) != 1:
            raise ValueError('Author Cargo.toml does not have the expected shared core dependency')
        content = original.replace(CORE_DEPENDENCY.encode(), STAGED_DEPENDENCY.encode())
    target.write_bytes(content)
    return {'path': 'author/' + relative, 'repoPath': source.relative_to(REPO).as_posix(),
            'sourceSha256': sha256(original), 'stagedSha256': sha256(content)}


def copy_workflows(paths, root):
    (root / 'workflows').mkdir()
    entries = {}
    for path in paths:
        content = path.read_bytes()
        if not isinstance(json.loads(content), dict):
            raise ValueError(f'An authored workflow must be a JSON object: {path.name}')
        fingerprint = sha256(content)
        if fingerprint in entries:
            raise ValueError(f'Repeated workflow source SHA-256: {fingerprint}')
        name = f'workflows/{fingerprint}.json'
        (root / name).write_bytes(content)
        entries[fingerprint] = {'path': name, 'sourceName': path.name, **stamp(content)}
    write_json(root / 'workflow-templates.json', {'schemaVersion': 1,
               'templates': [{'sha256': fingerprint} for fingerprint in sorted(entries)]})
    return entries


def sdk_command(cli, operation, *args):
    result = subprocess.run([str(cli), operation, *map(str, args)], capture_output=True,
                            encoding='utf-8', timeout=600)
    if result.returncode:
        raise RuntimeError(f'SDK {operation} failed: {result.stdout or result.stderr}')
    return json.loads(result.stdout)


def archive_bundle(root, tracked):
    archive = root / 'author-bundle.zip'
    with zipfile.ZipFile(archive, 'x', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
        for name in sorted(tracked):
            entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.create_system = 3
            entry.external_attr = 0o100644 << 16
            bundle.writestr(entry, (root / name).read_bytes(), compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    result = sha256(archive.read_bytes())
    (root / 'author-bundle.sha256').write_bytes(f'{result}  {archive.name}\n'.encode('ascii'))
    return result


def build(args):
    out = args.out.resolve()
    if out.exists() or not out.parent.is_dir():
        raise ValueError('Output must be a new directory below an existing parent')
    suffix = '.exe' if os.name == 'nt' else ''
    cli = (args.sdk_runtime / f'bgx-adapter-v2{suffix}').resolve(strict=True)
    runtime_sdk = (args.sdk_runtime / 'adapter-sdk-v2').resolve(strict=True)
    worker = (runtime_sdk / f'bgx-adapter-v2-worker{suffix}').resolve(strict=True)
    sdk_source = args.sdk_source.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='comfy-author-', dir=out.parent) as temp:
        staged = Path(temp) / 'project'
        final = Path(temp) / 'bundle'
        final.mkdir()
        workflows = copy_workflows(args.workflow, final)
        sources = [staged_source(file, name, staged) for file, name in package_sources()]
        runtime = final / 'runtime'
        expected = read_json(staged / 'manifest.json')
        sdk_command(cli, 'build', '--sdk', sdk_source, '--project', staged)
        built = read_json(staged / 'manifest.json')
        if built['artifactSha256'] != expected['artifactSha256']:
            raise ValueError(f'Built Wasm digest {built["artifactSha256"]} differs from the pinned manifest '
                             f'{expected["artifactSha256"]}; review the change and use a new version')
        sdk_command(cli, 'pack', '--sdk', sdk_source, '--project', staged, '--out', runtime)
        check = sdk_command(cli, 'check', '--sdk', runtime_sdk, '--worker', worker, '--project', runtime)
        if check.get('status') != 'VALIDATED':
            raise RuntimeError('SDK did not validate the packed runtime')
        if sha256((runtime / built['entry']).read_bytes()) != built['artifactSha256']:
            raise ValueError('Packed Wasm does not match the author artifact identity')

        for row in sources:
            name = row['path'].removeprefix('author/')
            target = final / row['path']
            target.parent.mkdir(parents=True, exist_ok=True)
            content = (staged / name).read_bytes()
            if name == 'manifest.json':
                # SDK build emits a normalized manifest; retain the byte-exact input for rebuilding.
                content = (PACKAGE / name).read_bytes()
            if sha256(content) != row['stagedSha256']:
                raise ValueError(f'Author inputs changed during SDK build: {name}')
            target.write_bytes(content)
        files = {file.relative_to(final).as_posix(): stamp(file.read_bytes())
                 for file in sorted(final.rglob('*')) if file.is_file()}
        index = {'schemaVersion': 1, 'package': {key: built[key] for key in ('pluginId', 'version', 'artifactSha256')},
                 'sdk': {'cliSha256': sha256(cli.read_bytes()), 'workerSha256': sha256(worker.read_bytes()),
                         'templateLockSha256': sha256((sdk_source / 'templates/template-lock.json').read_bytes())},
                 'files': files, 'sources': sorted(sources, key=lambda row: row['path']), 'workflows': workflows}
        write_json(final / 'bundle-index.json', index)
        archive = archive_bundle(final, [*files, 'bundle-index.json'])
        write_json(final / 'sdk-check.json', check)
        if out.exists():
            raise ValueError('Output was created during the build')
        final.rename(out)
    return {'status': 'built', 'directory': str(out), 'authorBundleSha256': archive, **index['package']}


def find(args):
    bundle = args.bundle.resolve(strict=True)
    index = read_json(bundle / 'bundle-index.json')
    fingerprint = args.sha256.lower()
    if index.get('schemaVersion') != 1 or not re.fullmatch(r'[a-f0-9]{64}', fingerprint):
        raise ValueError('Expected an author bundle and a 64-digit source SHA-256')
    row = index['workflows'].get(fingerprint)
    name = f'workflows/{fingerprint}.json'
    if row is None or row['path'] != name:
        raise ValueError(f'Workflow source is not in the bundle: {fingerprint}')
    path = checked_path(bundle, name)
    actual = stamp(path.read_bytes())
    if actual != index['files'].get(name) or actual != {key: row[key] for key in actual} or actual['sha256'] != fingerprint:
        raise ValueError(f'Workflow source bytes changed: {fingerprint}')
    return {'status': 'found', 'path': str(path), **actual}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(required=True)
    assemble = commands.add_parser('build')
    assemble.add_argument('--sdk-source', type=Path, required=True)
    assemble.add_argument('--sdk-runtime', type=Path, required=True)
    assemble.add_argument('--out', type=Path, required=True)
    assemble.add_argument('--workflow', type=Path, required=True, action='append')
    assemble.set_defaults(run=build)
    lookup = commands.add_parser('find')
    lookup.add_argument('--bundle', type=Path, required=True)
    lookup.add_argument('--sha256', required=True)
    lookup.set_defaults(run=find)
    args = parser.parse_args()
    print(json.dumps(args.run(args), ensure_ascii=False, sort_keys=True))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
