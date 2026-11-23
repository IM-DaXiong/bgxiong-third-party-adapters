#!/usr/bin/env python3
"""Prepare an upload directory with locked adapter ZIPs and a signed online index.

Requires cryptography for the same Ed25519/Blake2b minisign verification used
by the host. Production private keys are read only by cargo tauri signer.
"""
from __future__ import annotations
import argparse
import base64
import datetime as dt
import hashlib
import importlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
INDEX_URL = 'https://r.bgxiong.com/client/ai-story/bgxiong-third-party-adapters/latest.json'
KEY_ID = 'adapter-index-20261119'
MAX_BYTES = 256 << 20
MAX_INDEX_BYTES = 1 << 20
SEMVER = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?\Z')
HEX = re.compile(r'[0-9a-f]{64}\Z')


# Rust semver::VersionReq syntax: comma conjunction, at most 32 comparators,
# ASCII-space trimming, numeric u64 components, and explicit prerelease/build IDs.
# No npm OR, hyphen range, loose leading zero, or unknown token is accepted.
_NUMBER = r'(?:0|[1-9][0-9]*)'
_PART = rf'(?:{_NUMBER}|[xX*])'
_IDS = r'[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*'
_REQUIREMENT = re.compile(
    rf'(?:>=|<=|>|<|=|~|\^)? *({_NUMBER})(?:\.({_PART}))?(?:\.({_PART}))?'
    rf'(?:-({_IDS}))?(?:\+({_IDS}))?\Z'
)
_U64_MAX = (1 << 64) - 1


def stable_version(value):
    if not isinstance(value, str) or not SEMVER.fullmatch(value):
        return False
    return all(int(component) <= _U64_MAX for component in value.split('+', 1)[0].split('.'))


def valid_host_range(value):
    if not isinstance(value, str):
        return False
    value = value.strip(' ')
    if value in ('*', 'x', 'X'):
        return True
    comparators = value.split(',')
    if not value or len(comparators) > 32:
        return False
    for comparator in comparators:
        match = _REQUIREMENT.fullmatch(comparator.strip(' '))
        if not match:
            return False
        major, minor, patch, pre, build = match.groups()
        for component in (major, minor, patch):
            if component is not None and component not in ('*', 'x', 'X') and int(component) > _U64_MAX:
                return False
        if minor in ('*', 'x', 'X') and patch is not None and patch not in ('*', 'x', 'X'):
            return False
        if (pre is not None or build is not None) and (patch is None or patch in ('*', 'x', 'X')):
            return False
        if pre and any(part.isdigit() and len(part) > 1 and part[0] == '0' for part in pre.split('.')):
            return False
    return True


def unsigned_environment(source=None):
    source = os.environ if source is None else source
    return {key: value for key, value in source.items()
            if not key.upper().startswith('TAURI_SIGNING_')
            and key.upper() != 'BGX_ADAPTER_INDEX_KEY_PASSWORD'}


def signer_environment(password, source=None):
    # Use only the independent adapter-index signing identity in the signing child.
    env = unsigned_environment(source)
    env['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'] = password
    return env


def read(path):
    return json.loads(Path(path).read_text(encoding='utf-8-sig'))


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode('utf-8')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def fields(value, expected):
    if not isinstance(value, dict) or set(value) != set(expected.split()):
        raise ValueError('Unexpected/missing fields: ' + str(expected))


def verify(payload, signature_b64, pubkey_b64):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    pub = base64.b64decode(pubkey_b64.strip(), validate=True).decode('utf-8').splitlines()
    sig = base64.b64decode(signature_b64.strip(), validate=True).decode('utf-8').splitlines()
    if len(pub) != 2 or len(sig) != 4 or not sig[0].startswith('untrusted comment: ') or not sig[2].startswith('trusted comment: '):
        raise ValueError('Invalid minisign text')
    pk, sd, global_sig = (base64.b64decode(x, validate=True) for x in (pub[1], sig[1], sig[3]))
    if len(pk) != 42 or pk[:2] != b'Ed' or len(sd) != 74 or len(global_sig) != 64 or pk[2:10] != sd[2:10] or sd[:2] not in (b'Ed', b'ED'):
        raise ValueError('Invalid minisign identity or size')
    key = Ed25519PublicKey.from_public_bytes(pk[10:])
    key.verify(sd[10:], hashlib.blake2b(payload, digest_size=64).digest() if sd[:2] == b'ED' else payload)
    key.verify(global_sig, sd[10:] + sig[2][len('trusted comment: '):].encode('utf-8'))


def source_fingerprints(locks):
    sources = {}
    for lock in locks:
        for row in lock['packages']:
            path = 'adapter-pack/manifest.json' if row.get('kind') == 'speechManaged' else 'package/manifest.json'
            manifest = next((f for f in row['files'] if f['path'] == path), None)
            if manifest:
                identity = (row['pluginId'], row['version'])
                if identity in sources and sources[identity] != manifest['sha256']:
                    raise ValueError('Conflicting source manifest fingerprints')
                sources[identity] = manifest['sha256']
    return sources


def evidence_path(relative):
    path = (ROOT / relative).resolve()
    if not path.is_relative_to(ROOT / 'release/evidence'):
        raise ValueError('Installation evidence must be in release/evidence')
    return path


def validate_first_install(entry, evidence, size, sha256):
    plugin, version = entry['pluginId'], entry['version']
    if evidence.get('kind') == 'ownerAuthorizedWithoutFirstInstall':
        fields(evidence, 'schemaVersion kind authorization packages')
        target = dict(pluginId=plugin, version=version, hostVersionRange=entry['hostVersionRange'],
                      targetZipBytes=size, targetZipSha256=sha256)
        if 'resourceVersion' in entry:
            target['resourceVersion'] = entry['resourceVersion']
        if (evidence['schemaVersion'] == 1 and isinstance(evidence['authorization'], str)
                and evidence['authorization'].strip() and isinstance(evidence['packages'], list)
                and evidence['packages'].count(target) == 1 and not entry['upgradeFrom']):
            return
        raise ValueError(f'Owner authorization does not cover this exact ZIP and host: {plugin}@{version}')
    if (evidence.get('newInstallQualified') is not True or evidence.get('exitCode') != 0
            or (evidence.get('pluginId'), evidence.get('version')) != (plugin, version)
            or evidence.get('targetZipSha256') != sha256 or evidence.get('targetZipBytes') != size):
        raise ValueError(f'First-install evidence does not qualify exact ZIP: {plugin}@{version}')


def validated_packages(args, zip_directory=None):
    if zip_directory is None:
        zip_directory = ROOT/'artifacts/cdn'
    current = read(ROOT/'release/current.json')
    eligible = read(ROOT/'release/online-releases.json')
    fields(eligible, 'schemaVersion packages')
    if eligible['schemaVersion'] != 1 or not isinstance(eligible['packages'], list) or not eligible['packages']:
        raise ValueError('No installation or upgrade eligibility is recorded')
    evidence_by_path = {
        entry['evidence']: read(evidence_path(entry['evidence'])) for entry in eligible['packages']
    }
    source_locks = {evidence_path(evidence['sourceLock']) for evidence in evidence_by_path.values()
                    if evidence.get('sourceLock')}
    sources = source_fingerprints([current] + [read(p) for p in source_locks.union(args.source_lock)])
    inventory = {(p['pluginId'], p['version']): p for p in current['packages']}
    seen, packages = set(), []
    for entry in eligible['packages']:
        resource = entry.get('resourceVersion')
        fields(entry, 'pluginId version newInstall hostVersionRange upgradeFrom evidence'
               + (' resourceVersion' if resource is not None else ''))
        plugin, version = entry['pluginId'], entry['version']
        if not isinstance(plugin, str) or not re.fullmatch(r'[a-z0-9._-]+', plugin) or not stable_version(version):
            raise ValueError('Invalid package identity or prerelease')
        identity = (plugin, version)
        if identity in seen:
            raise ValueError('Duplicate online package')
        seen.add(identity)
        locked = inventory.get(identity)
        managed = resource is not None
        expected_kind = 'speechManaged' if managed else 'installable'
        if not locked or locked['kind'] != expected_kind:
            raise ValueError(f'Online qualification is out of date for {plugin}@{version}; '
                             'register evidence for the current ZIP before signing latest.json')
        if type(entry['newInstall']) is not bool or not isinstance(entry['upgradeFrom'], list):
            raise ValueError('Invalid online eligibility types')
        host = entry['hostVersionRange']
        if not valid_host_range(host):
            raise ValueError('Invalid hostVersionRange')
        evidence = evidence_by_path[entry['evidence']]
        owner_first_install = evidence.get('kind') == 'ownerAuthorizedWithoutFirstInstall'
        if managed and (not stable_version(resource)
                        or (owner_first_install and not (host.startswith('>=') and stable_version(host[2:])))
                        or (not owner_first_install and (
                            evidence.get('managedSpeechQualified') is not True
                            or evidence.get('resourceVersion') != resource
                            or not stable_version(evidence.get('hostVersion'))
                            or host != '>=' + evidence['hostVersion']))):
            raise ValueError(f'Managed speech requires exact host/resource qualification: {plugin}@{version}')
        for source in entry['upgradeFrom']:
            fields(source, 'version manifestSha256')
            if not HEX.fullmatch(source['manifestSha256']) or sources.get((plugin, source['version'])) != source['manifestSha256']:
                raise ValueError('Source fingerprint missing/mismatched; provide --source-lock')
        # Every locked input is rechecked: an existing ZIP alone is not authority.
        for item in locked['files']:
            source = (ROOT/item['source']).resolve()
            if not source.is_relative_to(ROOT) or digest(source.read_bytes()) != item['sha256']:
                raise ValueError('Current release lock source changed')
        name = f'{plugin}-{version}.zip'
        zip_path = zip_directory/name
        data = zip_path.read_bytes()
        if not data or len(data) > MAX_BYTES:
            raise ValueError('Package byte limit exceeded')
        with zipfile.ZipFile(zip_path) as z:
            expected = {f['path']: f for f in locked['files'] if Path(f['path']).name != 'WHERE-TO-INSTALL.txt'}
            install_files = {'provisioning.json', 'prepare-resources.ps1'} if managed else {'adapter-install.json'}
            if len(z.namelist()) != len(expected) or set(z.namelist()) != set(expected) or not install_files <= expected.keys():
                raise ValueError('ZIP inventory/envelope differs from lock')
            for path, f in expected.items():
                if digest(z.read(path)) != f['sha256']:
                    raise ValueError('ZIP contents differ from lock')
            manifest = json.loads(z.read('adapter-pack/manifest.json' if managed else 'package/manifest.json'))
            if (manifest['pluginId'], manifest['version']) != identity:
                raise ValueError('ZIP manifest identity differs')
            if managed:
                provisioning = json.loads(z.read('provisioning.json'))
                if (provisioning['pluginId'], provisioning['pluginVersion'], provisioning['version']) != (plugin, version, resource):
                    raise ValueError('ZIP managed speech resource identity differs')
        sha256 = digest(data)
        if evidence.get('kind') == 'ownerAuthorizedWithoutFirstInstall' and not entry['newInstall']:
            raise ValueError(f'Owner first-install authorization cannot be used for upgrade-only entry: {plugin}@{version}')
        if entry['newInstall']:
            validate_first_install(entry, evidence, len(data), sha256)
        if managed and not owner_first_install and (evidence.get('targetZipSha256') != sha256 or evidence.get('targetZipBytes') != len(data)
                        or evidence.get('upgradeFrom', []) != entry['upgradeFrom']):
            raise ValueError(f'Managed speech evidence does not match exact ZIP or upgrade sources: {plugin}@{version}')
        dependencies = [{'kind': 'sdkSpeech', 'resourceVersion': resource}] if managed else []
        packages.append({'pluginId':plugin,'version':version,'displayName':manifest['displayName'],
            'newInstall':entry['newInstall'],'hostVersionRange':host,'protocolVersion':2,
            'upgradeFrom':entry['upgradeFrom'],'dependencies':dependencies,'file':name,'bytes':len(data),'sha256':sha256})
    return sorted(packages, key=lambda p:(p['pluginId'],p['version']))


def verified_index(latest, public):
    if len(latest) > MAX_INDEX_BYTES:
        raise ValueError('Signed index exceeds maximum size')
    envelope = json.loads(latest)
    fields(envelope, 'keyId payloadB64 signatureB64')
    if envelope['keyId'] != KEY_ID:
        raise ValueError('Unexpected keyId')
    payload = base64.b64decode(envelope['payloadB64'], validate=True)
    verify(payload, envelope['signatureB64'], public)
    value = json.loads(payload)
    fields(value, 'schemaVersion sequence publishedAt packages')
    if value['schemaVersion'] != 1 or type(value['sequence']) is not int or not 1 <= value['sequence'] <= _U64_MAX:
        raise ValueError('Invalid signed index schema or sequence')
    return value, digest(payload)


def public_index_bytes():
    request = urllib.request.Request(INDEX_URL, headers={'Cache-Control': 'no-cache'})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read(MAX_INDEX_BYTES + 1)


def prepare_index(packages, receipt, public, latest_path):
    live = public_index_bytes()
    live_value, live_sha = verified_index(live, public)
    sequence = live_value['sequence']
    if sequence < receipt['sequence'] or (sequence == receipt['sequence'] and live_sha != receipt['payloadSha256']):
        raise ValueError('Public index does not match the publication receipt or is older')
    published = {row['file']: row['sha256'] for row in live_value['packages']}
    for package in packages:
        if package['file'] in published and published[package['file']] != package['sha256']:
            raise ValueError(f"Attempt to change published ZIP {package['file']}")
    if latest_path.is_file():
        local = latest_path.read_bytes()
        local_value, local_sha = verified_index(local, public)
        if local_value['sequence'] == sequence and local_sha != live_sha:
            raise ValueError('Local and public index have different payloads at the same sequence')
        if local_value['sequence'] > sequence and local_value['packages'] == packages:
            return local, local_value['sequence']
        sequence = max(sequence, local_value['sequence'])
    if sequence == live_value['sequence'] and live_value['packages'] == packages:
        return live, sequence
    return None, sequence + 1


def prepare_local_index(packages, receipt, public, latest_path):
    latest = latest_path.read_bytes()
    baseline, payload_sha = verified_index(latest, public)
    sequence = baseline['sequence']
    if sequence < receipt['sequence'] or (sequence == receipt['sequence'] and payload_sha != receipt['payloadSha256']):
        raise ValueError('Local signed index differs from the last verified publication receipt')
    published = {row['file']: row['sha256'] for row in baseline['packages']}
    for package in packages:
        if package['file'] in published and package['sha256'] != published[package['file']]:
            raise ValueError(f"Attempt to change signed ZIP {package['file']}")
    if baseline['packages'] == packages:
        return latest, sequence
    return None, sequence + 1


def replace_file(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temp:
        temporary = Path(temp.name)
        temp.write(data)
    try:
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


class InvalidSigningPassword(ValueError):
    pass


def sign_index(args, packages, sequence, public, password, workdir):
    if not args.private_key_path or not args.private_key_path.is_file():
        raise ValueError('--private-key-path must name the separately managed private key')
    private_key = args.private_key_path.resolve()
    if any((parent / '.git').exists() for parent in private_key.parents):
        raise ValueError('Private key must not be inside any Git repository')
    value = {'schemaVersion': 1, 'sequence': sequence, 'packages': packages,
             'publishedAt': dt.datetime.now(dt.timezone(dt.timedelta(hours=8))).isoformat(timespec='seconds')}
    payload = canonical(value)
    with tempfile.TemporaryDirectory(dir=workdir, prefix='.sign-') as temporary:
        path = Path(temporary) / 'payload.json'
        path.write_bytes(payload)
        result = subprocess.run(['cargo', 'tauri', 'signer', 'sign', '--private-key-path', str(private_key), str(path)],
                                cwd=ROOT, env=signer_environment(password), capture_output=True)
        if result.returncode:
            if b'Wrong password for that key' in result.stderr:
                raise InvalidSigningPassword('Signing password does not match the adapter-index private key')
            raise ValueError('cargo tauri signer sign failed (secret-bearing signer output intentionally suppressed)')
        signature = path.with_suffix('.json.sig').read_text(encoding='utf-8').strip()
    envelope = canonical({'keyId': KEY_ID, 'payloadB64': base64.b64encode(payload).decode('ascii'), 'signatureB64': signature})
    verified_index(envelope, public)
    return envelope


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--commit-receipt', action='store_true', help='only after approval and successful public read-back')
    parser.add_argument('--prepare-upload', action='store_true', help='pack, sign when needed and stage one upload directory')
    parser.add_argument('--require-new-install', action='append', default=[], metavar='pluginId@version')
    parser.add_argument('--local-only', action='store_true',
                        help='verify the assembled ZIPs and use the local signed sequence without contacting CDN')
    parser.add_argument('--source-lock', type=Path, action='append', default=[])
    parser.add_argument('--private-key-path', type=Path)
    parser.add_argument('--verify-signing-password', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--pubkey', help='public .pub file path')
    args = parser.parse_args()
    try:
        if args.require_new_install and not args.prepare_upload:
            raise ValueError('--require-new-install requires --prepare-upload')
        if args.prepare_upload and args.commit_receipt:
            raise ValueError('--prepare-upload and --commit-receipt are separate modes')
        if args.local_only and not args.prepare_upload:
            raise ValueError('--local-only requires --prepare-upload')
        if not args.prepare_upload:
            return run_index(args)
        packer = importlib.import_module('pack-cdn-directory')
        with tempfile.TemporaryDirectory(prefix='bgx-cdn-index-') as temporary:
            candidate = Path(temporary)
            packer.assemble(packer.locked_packages(), candidate)
            return run_index(args, candidate, packer)
    except InvalidSigningPassword as exc:
        print(str(exc), file=sys.stderr)
        return 4
    except Exception as exc:
        print(f'Online index not published: {exc}',file=sys.stderr)
        return 1


def run_index(args, zip_directory=None, packer=None):
    packages = validated_packages(args, zip_directory)
    new_install = {f'{p["pluginId"]}@{p["version"]}' for p in packages if p['newInstall']}
    missing = set(args.require_new_install) - new_install
    if missing:
        raise ValueError('Missing first-install qualification: ' + ', '.join(sorted(missing)))
    receipt_path = ROOT/'release/online-index-receipt.json'
    receipt = read(receipt_path)
    fields(receipt, 'sequence payloadSha256 publishedAt')
    if type(receipt['sequence']) is not int or receipt['sequence'] < 1:
        raise ValueError('Invalid sequence receipt')
    if args.check and not args.prepare_upload:
        print(f'Validated {len(packages)} online packages')
        return 0
    if not args.pubkey:
        raise ValueError('--pubkey is required')
    public = Path(args.pubkey).read_text(encoding='utf-8-sig').strip()
    output = ROOT/'artifacts/cdn-index'
    latest_path = output/'latest.json'
    if args.commit_receipt:
        latest = latest_path.read_bytes()
        value, payload_sha = verified_index(latest, public)
        if value['sequence'] < receipt['sequence'] or value['packages'] != packages:
            raise ValueError('Stale signed index; eligibility or receipt changed')
        if public_index_bytes() != latest:
            raise ValueError('Public CDN bytes differ; receipt was not changed')
        if value['sequence'] == receipt['sequence'] and payload_sha != receipt['payloadSha256']:
            raise ValueError('Public payload changed at the same receipt sequence')
        updated = {'sequence': value['sequence'], 'payloadSha256': payload_sha, 'publishedAt': value['publishedAt']}
        replace_file(receipt_path, canonical(updated)+b'\n')
        print('Receipt committed after public byte and signature verification')
        return 0
    prepare = prepare_local_index if args.local_only else prepare_index
    index, sequence = prepare(packages, receipt, public, latest_path)
    if args.check:
        print(f'Validated {len(packages)} online packages; sequence {sequence}; signing required: {index is None}')
        return 0
    if index is None or args.verify_signing_password:
        password = os.environ.get('BGX_ADAPTER_INDEX_KEY_PASSWORD')
        if not password:
            print('BGX_ADAPTER_INDEX_KEY_PASSWORD is required for the new index', file=sys.stderr)
            return 3
        output.mkdir(parents=True, exist_ok=True)
        signed = sign_index(args, packages, sequence, public, password, output)
        if index is None:
            index = signed
    replace_file(latest_path, index)
    print(f'Verified index sequence {sequence}')
    if args.prepare_upload:
        packer.publish(packer.locked_packages(), local_only=args.local_only, required=args.require_new_install,
                       source_locks=args.source_lock, signed_index=index)
    return 0


if __name__ == '__main__':
    sys.exit(main())
