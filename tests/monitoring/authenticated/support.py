"""Owned TLS fixtures and bounded HTTPS receiver; no production receiver claim."""

import base64
import hashlib
import hmac
import http.client
import http.server
import json
import os
from pathlib import Path
import queue
import re
import secrets
import shutil
import socket
import ssl
import stat
import subprocess
import threading
import time
import urllib.parse


MAX_BODY = 65536
MAX_RESPONSE = 1048576
MAX_POLICY = 4096


def _secret(value):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value) is not None


def _private_write(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as output:
        output.write(data)


def make_pki(directory, openssl):
    """Create a new private PKI directory using the supplied existing OpenSSL."""
    directory = Path(directory)
    parent = directory.parent.resolve(strict=True)
    directory = parent / directory.name
    directory.mkdir(mode=0o700)  # Exclusive: never reuse another fixture's keys.
    environment = {key: value for key, value in os.environ.items()
                   if key.lower() in {'systemroot', 'windir', 'path', 'temp', 'tmp', 'tmpdir'}}

    def run(*args):
        try:
            result = subprocess.run([str(openssl), *map(str, args)], env=environment,
                                    stdin=subprocess.DEVNULL, capture_output=True, timeout=15,
                                    creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        except (OSError, subprocess.TimeoutExpired):
            raise RuntimeError('Owned test certificate generation failed') from None
        if result.returncode:
            raise RuntimeError('Owned test certificate generation failed')

    def quoted(path):
        text = path.as_posix()
        if any(ord(char) < 32 for char in text):
            raise ValueError('Unsupported test PKI path')
        return '"' + text.replace('\\', '\\\\').replace('"', '\\"') + '"'

    try:
        req_config = directory / 'request.cnf'
        _private_write(req_config, b'''[req]
prompt = no
distinguished_name = dn
[dn]
CN = Owned Monitoring Test
[ca_ext]
basicConstraints = critical,CA:TRUE,pathlen:0
keyUsage = critical,keyCertSign,cRLSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always
''')
        for name in ('ca', 'wrong-ca'):
            run('req', '-new', '-x509', '-newkey', 'rsa:2048', '-nodes', '-sha256', '-days', '2',
                '-config', req_config, '-extensions', 'ca_ext', '-subj', '/CN=Owned ' + name,
                '-keyout', directory / (name + '.key'), '-out', directory / (name + '.pem'))
        (directory / 'issued').mkdir(mode=0o700)
        _private_write(directory / 'index.txt', b'')
        _private_write(directory / 'serial', b'1000\n')
        ca_config = directory / 'issuer.cnf'
        config = f'''[ca]
default_ca = issuer
[issuer]
database = {quoted(directory / 'index.txt')}
new_certs_dir = {quoted(directory / 'issued')}
certificate = {quoted(directory / 'ca.pem')}
private_key = {quoted(directory / 'ca.key')}
serial = {quoted(directory / 'serial')}
default_md = sha256
default_days = 2
policy = subject_policy
unique_subject = no
copy_extensions = none
x509_extensions = server
[subject_policy]
commonName = supplied
[server]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:localhost
[wrong_server]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:wrong.invalid
'''
        _private_write(ca_config, config.encode('utf-8'))
        for name in ('server', 'wrong-server', 'expired-server'):
            key, csr, cert = (directory / (name + extension) for extension in ('.key', '.csr', '.pem'))
            run('req', '-new', '-newkey', 'rsa:2048', '-nodes', '-sha256', '-config', req_config,
                '-subj', '/CN=localhost', '-keyout', key, '-out', csr)
            options = ['-extensions', 'wrong_server'] if name == 'wrong-server' else []
            if name == 'expired-server':
                options += ['-startdate', '20200101000000Z', '-enddate', '20200102000000Z']
            run('ca', '-batch', '-notext', '-config', ca_config, '-in', csr, '-out', cert, *options)
        for path in directory.rglob('*'):
            if path.is_file():
                path.chmod(0o600)
        return {
            'ca': directory / 'ca.pem', 'cert': directory / 'server.pem', 'key': directory / 'server.key',
            'wrong_ca': directory / 'wrong-ca.pem', 'wrong_cert': directory / 'wrong-server.pem',
            'wrong_key': directory / 'wrong-server.key', 'expired_cert': directory / 'expired-server.pem',
            'expired_key': directory / 'expired-server.key',
        }
    except BaseException:
        # Only the new directory created above is eligible for cleanup.
        if directory.parent == parent and directory.resolve() == directory and not directory.is_symlink():
            shutil.rmtree(directory)
        raise


def context(ca):
    verified = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    verified.minimum_version = ssl.TLSVersion.TLSv1_3
    verified.load_verify_locations(cafile=str(ca))
    return verified


def request(url, ca, *, basic=None, bearer=None, data=None):
    """One verified HTTPS request; never follow redirects or inherit proxies."""
    parsed = urllib.parse.urlsplit(url)
    if (parsed.scheme != 'https' or not parsed.hostname or parsed.username is not None
            or parsed.password is not None or parsed.fragment or (basic is not None and bearer is not None)):
        raise ValueError('Invalid owned HTTPS request')
    headers = {}
    if basic is not None:
        if (not isinstance(basic, tuple) or len(basic) != 2
                or not all(isinstance(value, str) and len(value) <= 1024
                           and not any(ord(char) < 32 or ord(char) == 127 for char in value) for value in basic)
                or ':' in basic[0]):
            raise ValueError('Invalid owned Basic credential')
        headers['Authorization'] = 'Basic ' + base64.b64encode(':'.join(basic).encode('utf-8')).decode('ascii')
    if bearer is not None:
        if (not isinstance(bearer, str) or len(bearer) > 1024
                or any(ord(char) < 32 or ord(char) == 127 for char in bearer)):
            raise ValueError('Invalid owned Bearer credential')
        headers['Authorization'] = 'Bearer ' + bearer
    if data is not None:
        if not isinstance(data, bytes) or len(data) > MAX_RESPONSE:
            raise ValueError('Invalid owned request body')
        headers['Content-Type'] = 'application/json'
    target = parsed.path or '/'
    if parsed.query:
        target += '?' + parsed.query
    client = http.client.HTTPSConnection(parsed.hostname, parsed.port or 443, context=context(ca), timeout=3)
    try:
        client.request('POST' if data is not None else 'GET', target, body=data, headers=headers)
        response = client.getresponse()
        body = response.read(MAX_RESPONSE + 1)
        if len(body) > MAX_RESPONSE:
            raise ValueError('Owned HTTPS response exceeded its bound')
        return response.status, body
    finally:
        client.close()


def _unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('Duplicate JSON key')
        result[key] = value
    return result


def _invalid_constant(_value):
    raise ValueError('Invalid JSON number')


def _json(data):
    return json.loads(data.decode('utf-8'), object_pairs_hook=_unique, parse_constant=_invalid_constant)


def _valid_policy(policy):
    return (isinstance(policy, dict)
            and set(policy) == {'token_sha256', 'active', 'not_before', 'expires_at'}
            and _secret(policy['token_sha256']) and type(policy['active']) is bool
            and all(type(policy[name]) is int and 0 <= policy[name] <= 2**63 - 1
                    for name in ('not_before', 'expires_at')))


def write_policy(path, token, *, active=True, not_before=0, expires_at=4102444800):
    if not _secret(token):
        raise ValueError('Invalid receiver credential')
    policy = {'token_sha256': hashlib.sha256(token.encode('ascii')).hexdigest(), 'active': active,
              'not_before': not_before, 'expires_at': expires_at}
    if not _valid_policy(policy):
        raise ValueError('Invalid receiver policy')
    path = Path(path)  # Keep the caller's pathname, including after replacement.
    if path.is_symlink():
        raise ValueError('Receiver policy must not be a symlink')
    temporary = path.with_name('.receiver-policy-' + secrets.token_hex(8) + '.tmp')
    try:
        _private_write(temporary, json.dumps(policy).encode('ascii'))
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def _load_policy(path):
    flags = (os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_BINARY', 0)
             | getattr(os, 'O_NONBLOCK', 0))
    if path.is_symlink():
        raise ValueError('Invalid receiver policy')
    with os.fdopen(os.open(path, flags), 'rb') as stream:
        metadata = os.fstat(stream.fileno())
        if (not stat.S_ISREG(metadata.st_mode) or metadata.st_size > MAX_POLICY
                or (os.name == 'posix' and (metadata.st_mode & 0o077 or not metadata.st_mode & 0o400))):
            raise ValueError('Invalid receiver policy')
        data = stream.read(MAX_POLICY + 1)
        if len(data) > MAX_POLICY:
            raise ValueError('Invalid receiver policy')
        policy = _json(data)
    if not _valid_policy(policy):
        raise ValueError('Invalid receiver policy')
    return policy


class _Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_args):
        pass

    def _reply(self, status, *, close=False):
        self.send_response(status)
        self.send_header('Content-Length', '0')
        self.send_header('Cache-Control', 'no-store')
        self.send_header('X-Content-Type-Options', 'nosniff')
        if status == 401:
            self.send_header('WWW-Authenticate', 'Bearer')
        if close:
            self.send_header('Connection', 'close')
            self.close_connection = True
        self.end_headers()

    def send_error(self, code, message=None, explain=None):
        self._reply(code, close=True)  # Never echo the request or private values.

    def do_GET(self):
        self._reply(405, close=True)

    def do_POST(self):
        if self.path != '/alerts':
            self._reply(404, close=True)
            return
        if sum(len(key) + len(value) for key, value in self.headers.items()) > 8192:
            self._reply(431, close=True)
            return
        authorization = self.headers.get_all('Authorization', [])
        if (len(authorization) != 1 or not authorization[0].startswith('Bearer ')
                or not _secret(authorization[0][7:])):
            self._reply(401, close=True)
            return
        try:
            policy = _load_policy(self.server.policy)
        except (OSError, ValueError, RecursionError):
            self._reply(503, close=True)
            return
        digest = hashlib.sha256(authorization[0][7:].encode('ascii')).hexdigest()
        now = time.time()
        if (not hmac.compare_digest(policy['token_sha256'], digest) or not policy['active']
                or not policy['not_before'] <= now < policy['expires_at']):
            self._reply(401, close=True)
            return
        lengths = self.headers.get_all('Content-Length', [])
        if (self.headers.get_all('Transfer-Encoding') or len(lengths) != 1
                or not re.fullmatch('[0-9]{1,8}', lengths[0])):
            self._reply(400, close=True)
            return
        length = int(lengths[0])
        if length > MAX_BODY:
            self._reply(413, close=True)
            return
        try:
            body = self.rfile.read(length)
            if len(body) != length:
                raise ValueError('Incomplete notification')
            payload = _json(body)
            if not isinstance(payload, dict) or not isinstance(payload.get('alerts'), list):
                raise ValueError('Invalid notification')
        except (OSError, ValueError, RecursionError):
            self._reply(400, close=True)
            return
        try:
            self.server.notifications.put_nowait(payload)
        except queue.Full:
            self._reply(503, close=True)
            return
        self._reply(200)


class _Server(http.server.HTTPServer):
    request_queue_size = 8

    def __init__(self, certificate, key, policy, notifications):
        self.tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.tls.minimum_version = ssl.TLSVersion.TLSv1_3
        self.tls.load_cert_chain(str(certificate), str(key))
        self.policy = policy
        self.notifications = notifications
        self.admission = threading.BoundedSemaphore(8)
        self.lock = threading.Lock()
        self.connections = {}
        self.workers = []
        super().__init__(('127.0.0.1', 0), _Handler)

    def get_request(self):
        stream, address = super().get_request()
        stream.settimeout(2)
        try:
            # Handshakes run inside admitted workers, never the accept thread.
            return self.tls.wrap_socket(stream, server_side=True, do_handshake_on_connect=False), address
        except BaseException:
            stream.close()
            raise

    def process_request(self, stream, address):
        if not self.admission.acquire(blocking=False):
            self.shutdown_request(stream)
            return
        with self.lock:
            self.connections[stream] = time.monotonic()
        worker = threading.Thread(target=self._handle, args=(stream, address),
                                  name='monitor-receiver-worker', daemon=False)
        self.workers.append(worker)
        try:
            worker.start()
        except BaseException:
            self.workers.remove(worker)
            with self.lock:
                self.connections.pop(stream, None)
            self.admission.release()
            self.shutdown_request(stream)
            raise

    def _handle(self, stream, address):
        try:
            stream.do_handshake()
            self.finish_request(stream, address)
        except Exception:
            # Owned peer disconnect/handshake/parser failures disclose no inputs.
            pass
        finally:
            self.shutdown_request(stream)
            with self.lock:
                self.connections.pop(stream, None)
            self.admission.release()

    @staticmethod
    def _close_immediately(stream):
        try:
            stream.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        stream.close()

    def service_actions(self):
        with self.lock:
            expired = [stream for stream, started in self.connections.items() if time.monotonic() - started >= 10]
        for stream in expired:
            self._close_immediately(stream)
        self.workers = [worker for worker in self.workers if worker.is_alive()]

    def server_close(self):
        super().server_close()
        with self.lock:
            streams = list(self.connections)
        for stream in streams:
            self._close_immediately(stream)
        for worker in self.workers:
            worker.join(timeout=3)
        if any(worker.is_alive() for worker in self.workers):
            raise RuntimeError('Owned receiver worker failed to stop')


class Receiver:
    def __init__(self, cert, key, policy):
        self.cert, self.key, self.policy = Path(cert), Path(key), Path(policy)
        self.notifications = queue.Queue(maxsize=64)
        self._server = None
        self._thread = None

    def __enter__(self):
        self._server = _Server(self.cert, self.key, self.policy, self.notifications)
        self.port = self._server.server_port
        self.url = f'https://localhost:{self.port}/alerts'
        self._thread = threading.Thread(target=lambda: self._server.serve_forever(poll_interval=0.05),
                                        name='monitor-receiver-listener', daemon=False)
        try:
            self._thread.start()
        except BaseException:
            self._server.server_close()
            raise
        return self

    def __exit__(self, *_exception):
        self._server.shutdown()
        self._server.server_close()
        self._thread.join(timeout=3)
        if self._thread.is_alive():
            raise RuntimeError('Owned receiver listener failed to stop')
