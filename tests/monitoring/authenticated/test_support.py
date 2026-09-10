from contextlib import closing
import http.client
import importlib.util
import json
import os
from pathlib import Path
import queue
import shutil
import socket
import ssl
import tempfile
import threading
import time
import unittest


class SupportTests(unittest.TestCase):
    support = None

    def setUp(self):
        source = Path(__file__).with_name('support.py')
        self.assertTrue(source.is_file(), 'Owned TLS support is not implemented')
        if type(self).support is None:
            spec = importlib.util.spec_from_file_location('owned_tls_support', source)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            type(self).support = module
            directory = tempfile.TemporaryDirectory(prefix='monitor-tls-tests-')
            type(self).addClassCleanup(directory.cleanup)
            openssl = os.environ.get('OPENSSL_BIN') or shutil.which('openssl')
            if not openssl:
                openssl = 'C:/Program Files/Git/usr/bin/openssl.exe'
            self.assertTrue(Path(openssl).is_file(), 'Existing OpenSSL executable is required')
            type(self).pki = module.make_pki(Path(directory.name) / 'pki', openssl)
        temporary = tempfile.TemporaryDirectory(prefix='monitor-policy-test-')
        self.addCleanup(temporary.cleanup)
        self.policy = Path(temporary.name) / 'receiver.json'
        self.token = 'a' * 64
        self.support.write_policy(self.policy, self.token)

    def receiver(self, certificate='cert', key='key'):
        return self.support.Receiver(self.pki[certificate], self.pki[key], self.policy)

    def send(self, receiver, token=None, body=b'{"alerts":[]}'):
        return self.support.request(receiver.url, self.pki['ca'], bearer=token, data=body)

    def test_valid_peer_and_token_deliver_bounded_notification(self):
        context = self.support.context(self.pki['ca'])
        self.assertEqual(context.minimum_version, ssl.TLSVersion.TLSv1_3)
        self.assertTrue(context.check_hostname)
        self.assertEqual(context.verify_mode, ssl.CERT_REQUIRED)
        with self.receiver() as receiver:
            payload = {'alerts': [{'status': 'firing', 'labels': {'alertname': 'OwnedFixture'}}]}
            status, body = self.send(receiver, self.token, json.dumps(payload).encode())
            self.assertEqual((status, body), (200, b''))
            self.assertEqual(receiver.notifications.get(timeout=1), payload)
            self.assertGreater(receiver.notifications.maxsize, 0)

    def test_missing_wrong_revoked_expired_future_and_restored_policies(self):
        with self.receiver() as receiver:
            self.assertEqual(self.send(receiver)[0], 401)
            self.assertEqual(self.send(receiver, 'b' * 64)[0], 401)
            for policy in ({'active': False}, {'expires_at': int(time.time()) - 1},
                           {'not_before': int(time.time()) + 60}):
                self.support.write_policy(self.policy, self.token, **policy)
                self.assertEqual(self.send(receiver, self.token)[0], 401)
            self.assertTrue(receiver.notifications.empty())
            self.support.write_policy(self.policy, self.token)
            self.assertEqual(self.send(receiver, self.token)[0], 200)

    def test_policy_reload_denies_a_previously_authorized_connection(self):
        with self.receiver() as receiver:
            client = http.client.HTTPSConnection('localhost', receiver.port,
                                                 context=self.support.context(self.pki['ca']), timeout=2)
            self.addCleanup(client.close)
            headers = {'Authorization': 'Bearer ' + self.token, 'Content-Type': 'application/json'}
            client.request('POST', '/alerts', body=b'{"alerts":[]}', headers=headers)
            reply = client.getresponse()
            self.assertEqual(reply.status, 200)
            reply.read()
            original_socket = client.sock
            self.assertIsNotNone(original_socket)
            self.support.write_policy(self.policy, self.token, active=False)
            self.assertIs(client.sock, original_socket)
            client.request('POST', '/alerts', body=b'{"alerts":[]}', headers=headers)
            reply = client.getresponse()
            self.assertEqual(reply.status, 401)
            reply.read()
            self.assertEqual(receiver.notifications.qsize(), 1)

    def test_unavailable_malformed_or_nonprivate_policy_fails_closed_and_recovers(self):
        with self.receiver() as receiver:
            self.policy.unlink()
            self.assertEqual(self.send(receiver, self.token)[0], 503)
            self.policy.write_text('{invalid')
            self.assertEqual(self.send(receiver, self.token)[0], 503)
            self.support.write_policy(self.policy, self.token)
            if os.name == 'posix':
                self.policy.chmod(0o644)
                self.assertEqual(self.send(receiver, self.token)[0], 503)
                self.policy.chmod(0o000)
                self.assertEqual(self.send(receiver, self.token)[0], 503)
                self.policy.chmod(0o600)
                self.policy.unlink()
                os.mkfifo(self.policy, 0o600)
                self.assertEqual(self.send(receiver, self.token)[0], 503)
                self.policy.unlink()
                self.support.write_policy(self.policy, self.token)
            self.assertEqual(self.send(receiver, self.token)[0], 200)

    def test_wrong_ca_hostname_and_expired_certificate_fail_tls_with_healthy_controls(self):
        with self.receiver() as receiver:
            self.assertEqual(self.send(receiver, self.token)[0], 200)
            with self.assertRaises(ssl.SSLCertVerificationError):
                self.support.request(receiver.url, self.pki['wrong_ca'], bearer=self.token, data=b'{"alerts":[]}')
            self.assertEqual(self.send(receiver, self.token)[0], 200)
        for certificate, key, code in [('wrong_cert', 'wrong_key', 62), ('expired_cert', 'expired_key', 10)]:
            with self.receiver(certificate, key) as receiver:
                with self.assertRaises(ssl.SSLCertVerificationError) as failed:
                    self.send(receiver, self.token)
                self.assertEqual(failed.exception.verify_code, code)
                self.assertTrue(receiver.notifications.empty())
        with self.receiver() as receiver:
            self.assertEqual(self.send(receiver, self.token)[0], 200)

    def test_duplicate_auth_and_invalid_or_excessive_json_do_not_enqueue(self):
        with self.receiver() as receiver:
            client = http.client.HTTPSConnection('localhost', receiver.port,
                                                 context=self.support.context(self.pki['ca']), timeout=2)
            self.addCleanup(client.close)
            client.putrequest('POST', '/alerts')
            for _ in range(2):
                client.putheader('Authorization', 'Bearer ' + self.token)
            client.putheader('Content-Length', '13')
            client.endheaders(b'{"alerts":[]}')
            reply = client.getresponse()
            self.assertEqual(reply.status, 401)
            self.assertEqual(reply.getheader('Cache-Control'), 'no-store')
            self.assertEqual(reply.getheader('X-Content-Type-Options'), 'nosniff')
            reply.read()
            for body in (b'not json', b'[]', b'{"alerts":{},"alerts":[]}', b'{"alerts":[NaN]}'):
                self.assertEqual(self.send(receiver, self.token, body)[0], 400)
            self.assertTrue(receiver.notifications.empty())
            self.assertEqual(self.send(receiver, self.token)[0], 200)

    def test_oversized_headers_and_uploads_are_rejected_and_recover(self):
        # A valid JSON document proves that the size limit, rather than JSON
        # parsing, prevents an oversized upload from reaching the queue.
        prefix, suffix = b'{"alerts":[],"padding":"', b'"}'
        oversized = prefix + b'x' * (65537 - len(prefix) - len(suffix)) + suffix
        with self.receiver() as receiver:
            with closing(http.client.HTTPSConnection('localhost', receiver.port,
                         context=self.support.context(self.pki['ca']), timeout=2)) as client:
                client.putrequest('POST', '/alerts')
                client.putheader('Authorization', 'Bearer ' + self.token)
                client.putheader('Content-Length', str(len(oversized)))
                client.endheaders()
                # The receiver must reject the declared length without waiting
                # for any body bytes or draining an arbitrarily large upload.
                reply = client.getresponse()
                self.assertEqual(reply.status, 413)
                self.assertEqual(reply.getheader('Connection'), 'close')
                reply.read()
            self.assertTrue(receiver.notifications.empty())
            for _ in range(3):
                try:
                    self.assertEqual(self.send(receiver, self.token, oversized)[0], 413)
                except (ConnectionResetError, ConnectionAbortedError, BrokenPipeError,
                        http.client.RemoteDisconnected, ssl.SSLEOFError):
                    # Closing with unread TLS body data can produce a reset
                    # before the client receives 413, particularly on Windows.
                    pass
                self.assertTrue(receiver.notifications.empty())
                self.assertEqual(self.send(receiver, self.token)[0], 200)
                self.assertEqual(receiver.notifications.get(timeout=1), {'alerts': []})
                self.assertTrue(receiver.notifications.empty())

    def test_full_notification_queue_returns_unavailable_and_recovers(self):
        with self.receiver() as receiver:
            for _ in range(receiver.notifications.maxsize):
                receiver.notifications.put_nowait({'alerts': []})
            self.assertEqual(self.send(receiver, self.token)[0], 503)
            receiver.notifications.get_nowait()
            self.assertEqual(self.send(receiver, self.token)[0], 200)

    def test_context_exit_closes_idle_tls_sockets_and_all_owned_threads(self):
        with self.receiver() as receiver:
            port = receiver.port
            client = socket.create_connection(('127.0.0.1', port), timeout=2)
            self.addCleanup(client.close)
            client.sendall(b'\x16')  # Deliberately incomplete owned TLS handshake.
            self.assertEqual(self.send(receiver, self.token)[0], 200)
        self.assertFalse(any(thread.name.startswith('monitor-receiver-') for thread in threading.enumerate()))
        with self.assertRaises(OSError):
            socket.create_connection(('127.0.0.1', port), timeout=1)
        try:
            self.assertEqual(client.recv(1), b'')
        except ConnectionResetError:
            pass

    def test_policy_is_hash_only_private_and_preserves_the_supplied_path(self):
        contents = self.policy.read_text()
        self.assertNotIn(self.token, contents)
        self.assertEqual(set(json.loads(contents)), {'token_sha256', 'active', 'not_before', 'expires_at'})
        if os.name == 'posix':
            self.assertEqual(self.policy.stat().st_mode & 0o077, 0)
        with self.receiver() as receiver:
            moved = self.policy.with_name('old-policy.json')
            self.policy.rename(moved)
            self.assertEqual(self.send(receiver, self.token)[0], 503)
            self.support.write_policy(self.policy, self.token)
            self.assertEqual(self.send(receiver, self.token)[0], 200)


if __name__ == '__main__':
    unittest.main()
