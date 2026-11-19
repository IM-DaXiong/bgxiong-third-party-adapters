#!/usr/bin/env python3
"""Offline publisher syntax/environment regressions; no keys, signing, or network."""
import importlib.util
from pathlib import Path
import unittest

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


if __name__ == '__main__':
    unittest.main()
