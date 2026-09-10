"""Behavioral checks for the operator's authenticated monitoring renderer."""

import copy
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
PROFILE = ROOT / 'scripts/monitoring/auth_profile.py'


class ProfileTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='board-profile-test-')
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name).resolve()
        self.output = self.directory / 'profile'
        self.assertTrue(PROFILE.is_file(), 'Authenticated profile renderer is not implemented')
        spec = importlib.util.spec_from_file_location('auth_profile', PROFILE)
        self.profile = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.profile)
        self.values = {}
        for index, name in enumerate(('prom-operator', 'am-ingest', 'am-operator', 'receiver-token', 'scrape-token'), 1):
            value = f'{index:064x}'
            self.values[name] = value.encode()
            self.file(name, (value + ('\n' if index % 2 else '')).encode())
        for name in ('ca.pem', 'cert.pem', 'key.pem', 'am-ca.pem', 'am-cert.pem', 'am-key.pem', 'rules.yml'):
            self.file(name, b'Native tools validate certificate/key/rule syntax separately.\n')
        endpoint = {
            'listen': '127.0.0.1:9090', 'server_name': 'localhost',
            'ca_file': str(self.directory / 'ca.pem'),
            'cert_file': str(self.directory / 'cert.pem'),
            'key_file': str(self.directory / 'key.pem'),
            'operator_password_file': str(self.directory / 'prom-operator'),
        }
        self.manifest = {
            'prometheus': endpoint,
            'alertmanager': {**endpoint, 'listen': '127.0.0.1:9093',
                             'ca_file': str(self.directory / 'am-ca.pem'),
                             'cert_file': str(self.directory / 'am-cert.pem'),
                             'key_file': str(self.directory / 'am-key.pem'),
                             'ingest_password_file': str(self.directory / 'am-ingest'),
                             'operator_password_file': str(self.directory / 'am-operator')},
            'receiver': {'url': 'https://localhost:9095/alerts',
                         'ca_file': str(self.directory / 'ca.pem'),
                         'token_file': str(self.directory / 'receiver-token')},
            'scrapes': [{'job': 'board-public', 'target': '127.0.0.1:9191',
                         'token_file': str(self.directory / 'scrape-token')}],
            'rules_file': str(self.directory / 'rules.yml'),
        }

    def file(self, name, contents):
        path = self.directory / name
        path.write_bytes(contents)
        path.chmod(0o600)
        return path

    def rejected(self, manifest):
        with self.assertRaises(ValueError):
            self.profile.render(manifest, self.output)
        self.assertFalse(self.output.exists(), 'Invalid inputs created partial output')

    def test_render_authenticates_both_hops_and_native_apis_without_inline_secrets(self):
        import bcrypt
        before = copy.deepcopy(self.manifest)
        paths = self.profile.render(self.manifest, self.output)
        self.assertEqual(self.manifest, before)
        self.assertEqual(set(paths), {'prometheus', 'prometheus_web', 'alertmanager', 'alertmanager_web'})
        self.assertEqual({path.name for path in paths.values()}, {'prometheus.yml', 'prometheus-web.yml', 'alertmanager.yml', 'alertmanager-web.yml'})
        configs = {name: json.loads(path.read_text()) for name, path in paths.items()}
        prom = configs['prometheus']
        target = prom['alerting']['alertmanagers'][0]
        self.assertEqual(target['static_configs'], [{'targets': ['127.0.0.1:9093']}])
        self.assertEqual(target['scheme'], 'https')
        self.assertEqual(target['basic_auth'], {'username': 'prometheus', 'password_file': self.manifest['alertmanager']['ingest_password_file']})
        self.assertEqual(target['tls_config'], {'ca_file': self.manifest['alertmanager']['ca_file'], 'server_name': 'localhost', 'min_version': 'TLS13'})
        self.assertIs(target['follow_redirects'], False)
        self.assertEqual(prom['rule_files'], [self.manifest['rules_file']])
        scrape = prom['scrape_configs'][0]
        self.assertEqual(scrape['job_name'], 'board-public')
        self.assertEqual(scrape['static_configs'], [{'targets': ['127.0.0.1:9191']}])
        self.assertEqual(scrape['authorization'], {'type': 'Bearer', 'credentials_file': self.manifest['scrapes'][0]['token_file']})
        self.assertIs(scrape['follow_redirects'], False)
        receiver = configs['alertmanager']['receivers'][0]['webhook_configs'][0]
        self.assertEqual(receiver['url'], self.manifest['receiver']['url'])
        self.assertIs(receiver['send_resolved'], True)
        self.assertEqual(receiver['http_config']['authorization'], {'type': 'Bearer', 'credentials_file': self.manifest['receiver']['token_file']})
        self.assertEqual(receiver['http_config']['tls_config'], {'ca_file': self.manifest['receiver']['ca_file'], 'min_version': 'TLS13'})
        self.assertIs(receiver['http_config']['follow_redirects'], False)
        for name, expected in (
            ('prometheus_web', {'operator': self.values['prom-operator']}),
            ('alertmanager_web', {'operator': self.values['am-operator'], 'prometheus': self.values['am-ingest']}),
        ):
            web = configs[name]
            endpoint = self.manifest[name.removesuffix('_web')]
            self.assertEqual(web['tls_server_config'], {'cert_file': endpoint['cert_file'], 'key_file': endpoint['key_file'], 'min_version': 'TLS13'})
            self.assertEqual(set(web['basic_auth_users']), set(expected))
            for user, password in expected.items():
                digest = web['basic_auth_users'][user].encode()
                self.assertTrue(digest.startswith(b'$2b$12$'))
                self.assertTrue(bcrypt.checkpw(password, digest))
                self.assertFalse(bcrypt.checkpw(b'wrong', digest))
        for path in paths.values():
            self.assertEqual(path.parent, self.output)
            for credential in self.values.values():
                self.assertNotIn(credential, path.read_bytes())
            self.assertNotIn(b'insecure_skip_verify', path.read_bytes())
            if os.name == 'posix':
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
        if os.name == 'posix':
            self.assertEqual(stat.S_IMODE(self.output.stat().st_mode), 0o700)
            self.assertEqual(self.output.stat().st_uid, os.geteuid())

    def test_numeric_ipv6_loopback_targets_and_explicit_ip_server_names_are_supported(self):
        self.manifest['prometheus']['listen'] = '[::1]:9090'
        self.manifest['alertmanager']['listen'] = '[::1]:9093'
        self.manifest['alertmanager']['server_name'] = '::1'
        self.manifest['scrapes'][0]['target'] = '[::1]:9191'
        self.manifest['receiver']['url'] = 'https://[::1]:9095/alerts'
        paths = self.profile.render(self.manifest, self.output)
        prom = json.loads(paths['prometheus'].read_text())
        self.assertEqual(prom['alerting']['alertmanagers'][0]['static_configs'][0]['targets'], ['[::1]:9093'])
        self.assertEqual(prom['alerting']['alertmanagers'][0]['tls_config']['server_name'], '::1')

    def test_unknown_fields_wrong_types_and_duplicate_closed_jobs_are_rejected(self):
        cases = []
        for section in ('prometheus', 'alertmanager', 'receiver'):
            case = copy.deepcopy(self.manifest)
            case[section]['unknown'] = 'ignored-is-unsafe'
            cases.append(case)
            case = copy.deepcopy(self.manifest)
            case[section] = []
            cases.append(case)
        cases.extend(({**self.manifest, 'unknown': 1}, {**self.manifest, 'scrapes': []}, {**self.manifest, 'scrapes': {}}, {**self.manifest, 'rules_file': 1}, {**self.manifest, 'scrapes': self.manifest['scrapes'] * 2}))
        for field, value in (('job', 'board-anything'), ('job', False), ('target', ['127.0.0.1:9191']), ('token_file', None), ('extra', 'value')):
            case = copy.deepcopy(self.manifest)
            case['scrapes'][0][field] = value
            cases.append(case)
        for case in cases:
            with self.subTest(case=len(cases)):
                self.rejected(case)

    def test_insecure_or_ambiguous_endpoints_are_rejected_before_writes(self):
        for field, values in (
            ('listen', ('0.0.0.0:9090', 'localhost:9090', '127.0.0.1:0', '127.0.0.1:65536', '127.0.0.1:9093', '[::]:9090', '[::1%lo]:9090', '127.0.0.1:9090\n')),
            ('server_name', ('*.example.com', 'user@localhost', 'localhost:9090', 'https://localhost', '', 'localhost\n', '-bad.example')),
        ):
            for value in values:
                case = copy.deepcopy(self.manifest)
                case['prometheus'][field] = value
                self.rejected(case)
        for target in ('0.0.0.0:9191', 'localhost:9191', '192.0.2.1:9191', '[::]:9191'):
            case = copy.deepcopy(self.manifest)
            case['scrapes'][0]['target'] = target
            self.rejected(case)
        for url in ('http://localhost/alerts', 'https://user:secret@localhost/alerts', 'https://localhost', 'https://localhost/alerts?secret=x', 'https://localhost/alerts#fragment', 'https://localhost:0/alerts', 'https://localhost:99999/alerts', 'https://localhost/alerts\n'):
            case = copy.deepcopy(self.manifest)
            case['receiver']['url'] = url
            self.rejected(case)

    def test_all_credentials_are_distinct_and_have_exact_bounded_hex_contents(self):
        for value in (b'', b'a' * 63, b'a' * 65, b'A' * 64, b'a' * 64 + b'\n\n', b'a' * 64 + b'\r\n', b'a' * 64 + b' ', b'\x00' * 64):
            self.file('receiver-token', value)
            self.rejected(self.manifest)
        self.file('receiver-token', self.values['receiver-token'])
        self.file('receiver-token', self.values['am-ingest'])
        self.rejected(self.manifest)
        self.file('receiver-token', self.values['receiver-token'])
        for section, field in (('prometheus', 'operator_password_file'), ('alertmanager', 'ingest_password_file'), ('alertmanager', 'operator_password_file'), ('receiver', 'token_file')):
            case = copy.deepcopy(self.manifest)
            case[section][field] = self.manifest['scrapes'][0]['token_file']
            self.rejected(case)

    def test_missing_noncanonical_nonregular_and_oversized_input_paths_are_rejected(self):
        for value in ('relative.pem', str(self.directory / 'absent.pem'), str(self.directory / '..' / self.directory.name / 'ca.pem'), str(self.directory)):
            case = copy.deepcopy(self.manifest)
            case['prometheus']['ca_file'] = value
            self.rejected(case)
        for name, limit in (('key.pem', 128 * 1024), ('cert.pem', 128 * 1024), ('ca.pem', 128 * 1024), ('rules.yml', 256 * 1024)):
            before = (self.directory / name).read_bytes()
            self.file(name, b'x' * (limit + 1))
            self.rejected(self.manifest)
            self.file(name, before)
        link = self.directory / 'link.pem'
        try:
            link.symlink_to(self.directory / 'ca.pem')
        except OSError:
            return # Windows may not grant symlink creation to the test account.
        self.manifest['prometheus']['ca_file'] = str(link)
        self.rejected(self.manifest)

    @unittest.skipUnless(os.name == 'posix', 'POSIX permission bits')
    def test_group_or_other_secret_and_key_access_is_rejected(self):
        for name in ('prom-operator', 'am-ingest', 'am-operator', 'receiver-token', 'scrape-token', 'key.pem', 'am-key.pem'):
            path = self.directory / name
            path.chmod(0o640)
            self.rejected(self.manifest)
            path.chmod(0o600)

    def test_existing_output_is_preserved_and_partial_owned_output_is_removed(self):
        self.output.mkdir()
        marker = self.output / 'unrelated'
        marker.write_bytes(b'preserve me')
        with self.assertRaises(ValueError):
            self.profile.render(self.manifest, self.output)
        self.assertEqual(marker.read_bytes(), b'preserve me')
        marker.unlink()
        self.output.rmdir()
        original = self.profile.os.open
        writes = 0

        def disk_failure(path, flags, *args, **kwargs):
            nonlocal writes
            if flags & os.O_CREAT:
                writes += 1
                if writes == 2:
                    raise OSError('synthetic private disk failure')
            return original(path, flags, *args, **kwargs)

        with mock.patch.object(self.profile.os, 'open', side_effect=disk_failure):
            with self.assertRaises(ValueError):
                self.profile.render(self.manifest, self.output)
        self.assertGreaterEqual(writes, 2)
        self.assertFalse(self.output.exists())

    def test_cli_rejects_duplicate_json_keys_oversize_and_secret_inputs_without_echo(self):
        manifest_file = self.directory / 'manifest.json'
        private = 'credential-must-not-appear'
        for raw in ('{"receiver":1,"receiver":"' + private + '"}', '{"private":"' + 'x' * (64 * 1024) + '"}', '{"private":"' + private + '"}'):
            manifest_file.write_text(raw)
            result = subprocess.run([sys.executable, str(PROFILE), '--manifest', str(manifest_file), '--output', str(self.output)], capture_output=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn(private.encode(), result.stdout + result.stderr)
            self.assertNotIn(str(manifest_file).encode(), result.stdout + result.stderr)
            self.assertNotIn(b'Traceback', result.stderr)
            self.assertFalse(self.output.exists())
        result = subprocess.run([sys.executable, str(PROFILE), '--manifest', str(manifest_file), '--output', str(self.output), '--unknown', private], capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(private.encode(), result.stdout + result.stderr)
        self.assertNotIn(b'Traceback', result.stderr)


if __name__ == '__main__':
    unittest.main()
