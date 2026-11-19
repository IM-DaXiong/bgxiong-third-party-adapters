#!/usr/bin/env python3
"""Validate and sign the separate adapter online index; never upload anything.

Requires cryptography for the same Ed25519/Blake2b minisign verification used
by the host. Production private keys are read only by cargo tauri signer.
"""
from __future__ import annotations
import argparse
import base64
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
INDEX_URL = 'https://r.bgxiong.com/client/ai-story/bgxiong-third-party-adapters/latest.json'
KEY_ID = 'adapter-index-20261119'
MAX_BYTES = 256 << 20
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


def signer_environment(password, source=None):
    # Never let the application's existing signing identity compete with the
    # explicitly selected independent index key; never pass the source password on.
    source = os.environ if source is None else source
    env = {key: value for key, value in source.items()
           if not key.upper().startswith('TAURI_SIGNING_')
           and key.upper() != 'BGX_ADAPTER_INDEX_KEY_PASSWORD'}
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
            manifest = next((f for f in row['files'] if f['path'] == 'package/manifest.json'), None)
            if manifest:
                identity = (row['pluginId'], row['version'])
                if identity in sources and sources[identity] != manifest['sha256']:
                    raise ValueError('Conflicting source manifest fingerprints')
                sources[identity] = manifest['sha256']
    return sources


def validated_packages(args):
    current = read(ROOT/'release/current.json')
    eligible = read(ROOT/'release/online-releases.json')
    fields(eligible, 'schemaVersion packages')
    if eligible['schemaVersion'] != 1 or not isinstance(eligible['packages'], list) or not eligible['packages']:
        raise ValueError('No measured S1 package pair recorded; online eligibility is not release-ready')
    sources = source_fingerprints([current] + [read(p) for p in args.source_lock])
    inventory = {(p['pluginId'], p['version']): p for p in current['packages']}
    seen, packages = set(), []
    for entry in eligible['packages']:
        fields(entry, 'pluginId version newInstall hostVersionRange upgradeFrom evidence')
        plugin, version = entry['pluginId'], entry['version']
        if not isinstance(plugin, str) or not re.fullmatch(r'[a-z0-9._-]+', plugin) or not stable_version(version):
            raise ValueError('Invalid package identity or prerelease')
        identity = (plugin, version)
        if identity in seen:
            raise ValueError('Duplicate online package')
        seen.add(identity)
        locked = inventory.get(identity)
        if not locked or locked['kind'] != 'installable':
            raise ValueError('Online package is not installable in current lock')
        if type(entry['newInstall']) is not bool or not isinstance(entry['upgradeFrom'], list):
            raise ValueError('Invalid online eligibility types')
        host = entry['hostVersionRange']
        if not valid_host_range(host):
            raise ValueError('Invalid hostVersionRange')
        evidence = (ROOT / entry['evidence']).resolve()
        if not entry['evidence'] or not evidence.is_file() or not evidence.stat().st_size:
            raise ValueError('Missing measured installation/upgrade evidence')
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
        data = (ROOT/'artifacts/cdn'/name).read_bytes()
        if not data or len(data) > MAX_BYTES:
            raise ValueError('Package byte limit exceeded')
        with zipfile.ZipFile(ROOT/'artifacts/cdn'/name) as z:
            expected = {f['path']: f for f in locked['files'] if Path(f['path']).name != 'WHERE-TO-INSTALL.txt'}
            if len(z.namelist()) != len(expected) or set(z.namelist()) != set(expected) or 'adapter-install.json' not in expected:
                raise ValueError('ZIP inventory/envelope differs from lock')
            for path, f in expected.items():
                if digest(z.read(path)) != f['sha256']:
                    raise ValueError('ZIP contents differ from lock')
            manifest = json.loads(z.read('package/manifest.json'))
            if (manifest['pluginId'], manifest['version']) != identity:
                raise ValueError('ZIP manifest identity differs')
        packages.append({'pluginId':plugin,'version':version,'displayName':manifest['displayName'],
            'newInstall':entry['newInstall'],'hostVersionRange':host,'protocolVersion':2,
            'upgradeFrom':entry['upgradeFrom'],'dependencies':[],'file':name,'bytes':len(data),'sha256':digest(data)})
    return sorted(packages, key=lambda p:(p['pluginId'],p['version']))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--commit-receipt', action='store_true', help='only after approval and successful public read-back')
    parser.add_argument('--source-lock', type=Path, action='append', default=[])
    parser.add_argument('--private-key-path', type=Path)
    parser.add_argument('--pubkey', help='public .pub file path')
    args = parser.parse_args()
    try:
        packages = validated_packages(args)
        receipt_path = ROOT/'release/online-index-receipt.json'
        receipt = read(receipt_path)
        fields(receipt, 'sequence payloadSha256 publishedAt')
        if type(receipt['sequence']) is not int or receipt['sequence'] < 1:
            raise ValueError('Invalid sequence receipt')
        if args.check:
            print(f'Validated {len(packages)} online packages; next sequence {receipt["sequence"]+1}')
            return 0
        if not args.pubkey:
            raise ValueError('--pubkey is required')
        public = Path(args.pubkey).read_text(encoding='utf-8-sig').strip()
        output = ROOT/'artifacts/cdn-index'
        if args.commit_receipt:
            latest = (output/'latest.json').read_bytes()
            envelope = json.loads(latest)
            fields(envelope, 'keyId payloadB64 signatureB64')
            if envelope['keyId'] != KEY_ID:
                raise ValueError('Unexpected keyId')
            payload = base64.b64decode(envelope['payloadB64'], validate=True)
            verify(payload, envelope['signatureB64'], public)
            value = json.loads(payload)
            if value['sequence'] != receipt['sequence']+1 or value['packages'] != packages:
                raise ValueError('Stale signed index; eligibility or receipt changed')
            with urllib.request.urlopen(INDEX_URL, timeout=30) as response:
                if response.read((1<<20)+1) != latest:
                    raise ValueError('Public CDN bytes differ; receipt was not changed')
            updated={'sequence':value['sequence'],'payloadSha256':digest(payload),'publishedAt':value['publishedAt']}
            temporary=receipt_path.with_suffix('.json.tmp');temporary.write_bytes(canonical(updated)+b'\n');os.replace(temporary,receipt_path)
            print('Receipt committed after public byte and signature verification')
            return 0
        password = os.environ.get('BGX_ADAPTER_INDEX_KEY_PASSWORD')
        if not password:
            print('BGX_ADAPTER_INDEX_KEY_PASSWORD is required; no index signed',file=sys.stderr)
            return 3
        if not args.private_key_path or not args.private_key_path.is_file():
            raise ValueError('--private-key-path must name the separately managed private key')
        if any((parent / '.git').exists() for parent in args.private_key_path.resolve().parents):
            raise ValueError('Private key must not be inside any Git repository')
        output.mkdir(parents=True,exist_ok=True)
        value={'schemaVersion':1,'sequence':receipt['sequence']+1,
            'publishedAt':dt.datetime.now(dt.timezone(dt.timedelta(hours=8))).isoformat(timespec='seconds'),'packages':packages}
        payload=canonical(value);path=output/'payload.json';path.write_bytes(payload)
        env=signer_environment(password)
        result=subprocess.run(['cargo','tauri','signer','sign','--private-key-path',str(args.private_key_path),str(path)],cwd=ROOT,env=env,capture_output=True)
        if result.returncode:
            raise ValueError('cargo tauri signer sign failed (secret-bearing signer output intentionally suppressed)')
        signature=path.with_suffix('.json.sig').read_text(encoding='utf-8').strip()
        verify(payload,signature,public)
        envelope={'keyId':KEY_ID,'payloadB64':base64.b64encode(payload).decode('ascii'),'signatureB64':signature}
        tmp=output/'latest.json.tmp';tmp.write_bytes(canonical(envelope));os.replace(tmp,output/'latest.json')
        print(f'Index signed and self-verified, sequence {value["sequence"]}; NOT uploaded; receipt unchanged')
        return 0
    except Exception as exc:
        print(f'Online index not published: {exc}',file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
