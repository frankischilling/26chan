"""Owned Linux HTTPS round trip; called by the database integration test."""
import contextlib
import http.client
import json
import os
from pathlib import Path
import signal
import socket
import ssl
import subprocess
import sys
import tempfile
import time
import urllib.parse

ROOT = Path(__file__).resolve().parents[1]


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def stop(process):
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
            raise AssertionError('Owned service did not drain')


def main():
    binary, board = sys.argv[1:]
    assert sys.platform == 'linux'
    def interrupted(_signal, _frame):
        raise RuntimeError('Owned proxy qualification interrupted')
    signal.signal(signal.SIGTERM, interrupted)
    with tempfile.TemporaryDirectory(prefix='board-proxy-', dir='/tmp') as temporary:
        root = Path(temporary)
        port, unused_port, api_port = free_port(), free_port(), free_port()
        origin = f'https://127.0.0.1:{port}'
        path = root / 'public.sock'
        cert, key = root / 'public.crt', root / 'public.key'
        subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                        '-keyout', str(key), '-out', str(cert), '-days', '1',
                        '-subj', '/CN=127.0.0.1', '-addext', 'subjectAltName=IP:127.0.0.1'],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
        site = (ROOT / 'deploy/public-proxy.nginx.conf').read_text()
        for old, new in [('listen 443 ssl;', f'listen 127.0.0.1:{port} ssl;'),
                         ('boards.example.com', '127.0.0.1'),
                         ('/etc/paperboard/tls/public.crt', str(cert)),
                         ('/etc/paperboard/tls/public.key', str(key)),
                         ('/run/paperboard-public/public.sock', str(path)),
                         ('/var/log/nginx/board-public-error.log', str(root / 'edge-error.log'))]:
            assert old in site
            site = site.replace(old, new)
        (root / 'site.conf').write_text(site)
        for directory in ['body', 'proxy']:
            (root / directory).mkdir()
        config = root / 'nginx.conf'
        config.write_text(f'pid {root}/nginx.pid;\nerror_log {root}/nginx-error.log warn;\n'
                          f'events {{ worker_connections 64; }}\nhttp {{ access_log off; '
                          f'client_body_temp_path {root}/body; proxy_temp_path {root}/proxy; '
                          f'include {root}/site.conf; }}\n')
        environment = {'PATH': os.environ['PATH'], 'APP_ENV': 'development',
                       'DATABASE_URL': os.environ['TEST_PUBLIC_DATABASE_URL'],
                       'PUBLIC_ORIGIN': origin, 'MEDIA_ENABLED': 'false',
                       'BIND_ADDR': f'127.0.0.1:{unused_port}',
                       'API_ORIGIN': f'http://127.0.0.1:{api_port}',
                       'API_BIND_ADDR': f'127.0.0.1:{api_port}',
                       'PUBLIC_PROXY_SOCKET': str(path), 'PUBLIC_PROXY_UID': str(os.getuid()),
                       'PUBLIC_WRITES_PER_MINUTE': '3'}
        context = ssl.create_default_context(cafile=str(cert))

        def request(method, route, peer='127.0.0.2', fields=None, headers=None):
            connection = http.client.HTTPSConnection('127.0.0.1', port, timeout=10,
                                                     context=context, source_address=(peer, 0))
            try:
                body = urllib.parse.urlencode(fields) if fields is not None else None
                supplied = {'Origin': origin, 'Accept': 'application/json',
                            'Content-Type': 'application/x-www-form-urlencoded',
                            'X-Board-Client-IP': '127.0.0.2', 'X-Forwarded-For': '127.0.0.2',
                            'Forwarded': 'for=127.0.0.2'}
                supplied.update(headers or {})
                connection.request(method, route, body, supplied)
                response = connection.getresponse()
                return response.status, response.read()
            finally:
                connection.close()

        with contextlib.ExitStack() as cleanup:
            log = cleanup.enter_context((root / 'public.log').open('wb'))
            public = subprocess.Popen([binary], env=environment, stdout=log, stderr=log)
            cleanup.callback(stop, public)
            subprocess.run(['nginx', '-t', '-c', str(config)], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
            edge = subprocess.Popen(['nginx', '-c', str(config), '-g', 'daemon off;'],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            cleanup.callback(stop, edge)
            deadline = time.monotonic() + 15
            while True:
                assert public.poll() is None and edge.poll() is None, 'Owned service exited at startup'
                try:
                    if request('GET', '/readyz')[0] == 200:
                        break
                except (OSError, http.client.HTTPException):
                    pass
                assert time.monotonic() < deadline, 'Owned HTTPS listener did not become ready'
                time.sleep(0.05)
            with socket.socket() as probe:
                assert probe.connect_ex(('127.0.0.1', unused_port)) != 0, 'Unexpected public TCP listener'

            def submit(peer, parent, password='owned-different-password', expected=200):
                status, body = request('POST', f'/{board}/imgboard.php', peer,
                                       {'resto': parent, 'com': '[b]Owned proxy fixture[/b]', 'pwd': password})
                assert status == expected, (status, expected)
                if expected == 200:
                    value = json.loads(body)
                    assert 'error' not in value, value
                    return value['pid']

            result = {}
            result['op'] = submit('127.0.0.2', 0, 'owned-op-password')
            result['same'] = submit('127.0.0.2', result['op'])
            result['same_again'] = submit('127.0.0.2', result['op'])
            submit('127.0.0.2', result['op'], expected=429)
            result['other'] = submit('127.0.0.3', result['op'])
            result['password'] = submit('127.0.0.4', result['op'], 'owned-op-password')
            status, body = request('GET', f'/{board}/thread/{result["op"]}')
            assert status == 200 and b'class="mu-b"' in body
            api = http.client.HTTPConnection('127.0.0.1', api_port, timeout=5)
            try:
                api.request('POST', f'/{board}/imgboard.php', 'com=unavailable',
                            {'Origin': origin, 'X-Board-Client-IP': '127.0.0.2'})
                response = api.getresponse()
                assert response.status == 405
                response.read()
            finally:
                api.close()
        assert not path.exists(), 'Graceful shutdown left the owned socket behind'
        print(json.dumps(result))


if __name__ == '__main__':
    main()
