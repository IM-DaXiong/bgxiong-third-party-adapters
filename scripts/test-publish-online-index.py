#!/usr/bin/env python3
"""Offline publisher syntax/environment regressions; no keys, signing, or network."""
import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('publisher', Path(__file__).with_name('publish-online-index.py'))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class PublisherContractTests(unittest.TestCase):
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


if __name__ == '__main__':
    unittest.main()
