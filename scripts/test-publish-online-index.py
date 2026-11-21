#!/usr/bin/env python3
"""Offline publisher syntax/environment regressions; no keys, signing, or network."""
import importlib.util
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('publisher', Path(__file__).with_name('publish-online-index.py'))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class PublisherContractTests(unittest.TestCase):
    def test_owner_authorization_applies_to_exact_zip_and_host_only(self):
        entry = dict(pluginId='example', version='1.0.0', hostVersionRange='=0.6.1119',
                     upgradeFrom=[])
        digest = 'a' * 64
        approval = dict(schemaVersion=1, kind='ownerAuthorizedWithoutFirstInstall',
                        authorization='Unique current batch',
                        packages=[dict(pluginId='example', version='1.0.0',
                                       hostVersionRange='=0.6.1119', targetZipBytes=1024,
                                       targetZipSha256=digest)])
        publisher.validate_first_install(entry, approval, 1024, digest)
        for invalid_entry, size, sha in [
            (dict(entry, hostVersionRange='*'), 1024, digest),
            (dict(entry, version='1.0.1'), 1024, digest),
            (dict(entry, upgradeFrom=[{'version': '0.9.0'}]), 1024, digest),
            (entry, 1025, digest),
            (entry, 1024, 'b' * 64),
        ]:
            with self.subTest(entry=invalid_entry, size=size, digest=sha):
                with self.assertRaisesRegex(ValueError, 'does not cover this exact ZIP and host'):
                    publisher.validate_first_install(invalid_entry, approval, size, sha)

    def test_valid_host_ranges(self):
        for value in ['*', 'x', 'X', '^0.6', '>=0.6.0', '>= 0.6.0, <1.0.0',
                      '1', '1.2', '~1.2.3', '1.2.*', '1.x.*', '=1.2.3',
                      '<1.2.3-alpha.1+build.01', ' 1.2.3 ', '>1.*', '1.2.X']:
            with self.subTest(value=value):
                self.assertTrue(publisher.valid_host_range(value))

    def test_invalid_host_ranges(self):
        for value in ['', ' ', 'nonsense', '1.2.3 || 2', '1.2.3 - 2.0.0',
                      '>=1.0.0 <2.0.0', '01.2.3', '1.02', '1.2.03', '1.x.3',
                      '1.2.3-', '1.2.3+..', '1.2.3-alpha.01', '1.2-alpha',
                      '* , >=1', '>=*', '==1.2.3', '1.2.3,', '\t1.2.3',
                      '1.2.3\n', '18446744073709551616.0.0', None, 1]:
            with self.subTest(value=value):
                self.assertFalse(publisher.valid_host_range(value))

    def test_comparator_limit(self):
        self.assertTrue(publisher.valid_host_range(','.join(['>=1'] * 32)))
        self.assertFalse(publisher.valid_host_range(','.join(['>=1'] * 33)))

    def test_valid_stable_versions_and_build_metadata(self):
        for value in ['0.0.0', '1.2.3', '1.2.3+build.01', '18446744073709551615.0.0']:
            with self.subTest(value=value):
                self.assertTrue(publisher.stable_version(value))

    def test_rejects_prerelease_overflow_and_empty_build_identifiers(self):
        for value in ['1.2.3-alpha', '01.2.3', '1.2.3+.', '1.2.3+build..id',
                      '18446744073709551616.0.0', '1.2.3+id.', '1.2', None, 1]:
            with self.subTest(value=value):
                self.assertFalse(publisher.stable_version(value))

    def test_signer_never_inherits_application_signing_identity(self):
        env = publisher.signer_environment('index-test-sentinel', {
            'PATH': 'retained-test-path',
            'TAURI_SIGNING_PRIVATE_KEY': 'application-test-sentinel',
            'TAURI_SIGNING_PRIVATE_KEY_PASSWORD': 'application-password-test-sentinel',
            'BGX_ADAPTER_INDEX_KEY_PASSWORD': 'source-password-test-sentinel',
        })
        self.assertEqual(env, {'PATH': 'retained-test-path',
                              'TAURI_SIGNING_PRIVATE_KEY_PASSWORD': 'index-test-sentinel'})

    def test_online_and_prepared_indexes_determine_next_sequence(self):
        old = {'file': 'old-1.0.0.zip', 'sha256': 'a' * 64}
        new = {'file': 'new-1.0.0.zip', 'sha256': 'b' * 64}
        receipt = {'sequence': 2, 'payloadSha256': 'original'}

        def verified(raw, _public):
            return {
                b'public': ({'sequence': 4, 'packages': [old]}, 'published'),
                b'candidate': ({'sequence': 5, 'packages': [old, new]}, 'candidate'),
            }[raw]

        with TemporaryDirectory() as directory, \
                patch.object(publisher, 'public_index_bytes', return_value=b'public'), \
                patch.object(publisher, 'verified_index', side_effect=verified):
            latest = Path(directory) / 'latest.json'
            self.assertEqual(publisher.prepare_index([old], receipt, '', latest), (b'public', 4))
            self.assertEqual(publisher.prepare_index([old, new], receipt, '', latest), (None, 5))
            latest.write_bytes(b'candidate')
            self.assertEqual(publisher.prepare_index([old, new], receipt, '', latest), (b'candidate', 5))
            self.assertEqual(publisher.prepare_index([old], receipt, '', latest), (None, 6))
            with self.assertRaisesRegex(ValueError, 'change published ZIP'):
                publisher.prepare_index([dict(old, sha256='c' * 64)], receipt, '', latest)
            with self.assertRaisesRegex(ValueError, 'publication receipt'):
                publisher.prepare_index([old], dict(receipt, sequence=5), '', latest)


    def test_local_signing_checks_receipt_without_network_and_reuses_signed_payload(self):
        package = {'file': 'example-1.0.0.zip', 'sha256': 'a' * 64}
        receipt = {'sequence': 10, 'payloadSha256': 'verified'}
        with TemporaryDirectory() as directory, \
                patch.object(publisher, 'public_index_bytes', side_effect=AssertionError('network')), \
                patch.object(publisher, 'verified_index', return_value=(
                    {'sequence': 10, 'packages': [package]}, 'verified')):
            latest = Path(directory) / 'latest.json'
            latest.write_bytes(b'signed')
            self.assertEqual(publisher.prepare_local_index([package], receipt, '', latest), (b'signed', 10))
            self.assertEqual(publisher.prepare_local_index([], receipt, '', latest), (None, 11))
            with self.assertRaisesRegex(ValueError, 'publication receipt'):
                publisher.prepare_local_index([], dict(receipt, payloadSha256='other'), '', latest)
            with self.assertRaisesRegex(ValueError, 'change signed ZIP'):
                publisher.prepare_local_index([dict(package, sha256='b' * 64)], receipt, '', latest)


class UploadDirectoryTests(unittest.TestCase):
    def test_only_current_signed_batch_replaces_upload_directory(self):
        packer = importlib.import_module('pack-cdn-directory')
        verifier = importlib.import_module('stage-cdn-upload-directory')
        name = 'example-1.0.0.zip'
        locked = [(name, [('adapter-install.json', b'{}')])]
        with TemporaryDirectory() as directory:
            root = Path(directory)
            cdn = root / 'artifacts/cdn'
            workspace = root / 'artifacts/cdn-index'
            qualification = workspace / 'qualification'
            cdn.mkdir(parents=True)
            (cdn / 'previous.zip').write_bytes(b'previous upload')
            (cdn / 'latest.json').write_bytes(b'previous signature')
            release = root / 'release'
            release.mkdir()
            (release / 'online-index-receipt.json').write_text(
                json.dumps({'sequence': 1, 'payloadSha256': 'previous'}), encoding='utf-8')
            with patch.multiple(packer, ROOT=root, DEST=cdn, QUALIFICATION=qualification), \
                    patch.multiple(verifier, ROOT=root, INDEX_DIR=workspace):
                with self.assertRaisesRegex(packer.PackError, 'Missing production-signed latest.json'):
                    packer.publish(locked, local_only=True)
                packer.publish(locked, qualification=True)
                data = (qualification / name).read_bytes()
                expected = {'pluginId': 'example', 'version': '1.0.0', 'newInstall': True,
                            'file': name, 'bytes': len(data), 'sha256': packer.sha256(data)}

                def index(raw, _public):
                    package = dict(expected, newInstall=raw == b'current')
                    return {'sequence': 2, 'packages': [package]}, 'signed'

                publisher_fixture = SimpleNamespace(
                    KEY_ID='test', verified_index=index,
                    validated_packages=lambda _args, _staging: [expected])
                with patch.object(verifier, 'load_publisher', return_value=publisher_fixture), \
                        patch.object(verifier, 'host_public_trust', return_value=('test', 'test-public')):
                    (workspace / 'latest.json').write_bytes(b'unqualified')
                    with self.assertRaisesRegex(verifier.StageError, 'current online qualification'):
                        packer.publish(locked, local_only=True)
                    self.assertEqual((cdn / 'latest.json').read_bytes(), b'previous signature')
                    self.assertTrue((cdn / 'previous.zip').exists())
                    (workspace / 'latest.json').write_bytes(b'current')
                    packer.publish(locked, local_only=True)
                self.assertEqual({p.name for p in cdn.iterdir()}, {name, 'SHA256SUMS.txt', 'latest.json'})
                self.assertEqual((cdn / name).read_bytes(), data)
                self.assertEqual((cdn / 'latest.json').read_bytes(), b'current')


if __name__ == '__main__':
    unittest.main()
