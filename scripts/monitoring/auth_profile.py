"""Render a new private native HTTPS/auth profile without contacting any endpoint."""

import argparse
import ipaddress
import json
import os
from pathlib import Path
import re
import stat
import sys
from urllib.parse import urlsplit

MANIFEST_LIMIT = 64 * 1024
MATERIAL_LIMIT = 128 * 1024
RULES_LIMIT = 256 * 1024
JOBS = frozenset(('board-public', 'board-staff', 'board-media', 'board-monitor'))
ERROR = 'Authenticated monitoring profile could not be rendered.'


def _object(value, fields):
    if type(value) is not dict or value.keys() != set(fields):
        raise ValueError(ERROR)
    return value


def _text(value):
    if type(value) is not str or not value or any(ord(char) < 32 or ord(char) == 127 for char in value):
        raise ValueError(ERROR)
    return value


def _socket(value):
    value = _text(value)
    match = re.fullmatch(r'(?:\[([0-9a-fA-F:]+)\]|([0-9.]+)):([0-9]{1,5})', value)
    if not match:
        raise ValueError(ERROR)
    address = ipaddress.ip_address(match[1] or match[2])
    port = int(match[3])
    if not address.is_loopback or not 1 <= port <= 65535:
        raise ValueError(ERROR)
    return address, port


def _server_name(value):
    value = _text(value)
    if '%' in value or any(char.isspace() for char in value):
        raise ValueError(ERROR)
    try:
        ipaddress.ip_address(value)
        return
    except ValueError:
        pass
    name = value.removesuffix('.')
    if len(name) > 253 or not all(re.fullmatch(r'[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?', label) for label in name.split('.')):
        raise ValueError(ERROR)


def _receiver_url(value):
    value = _text(value)
    if any(char.isspace() for char in value) or '?' in value or '#' in value:
        raise ValueError(ERROR)
    url = urlsplit(value)
    if url.scheme != 'https' or not url.hostname or url.username is not None or url.password is not None or not url.path.startswith('/'):
        raise ValueError(ERROR)
    if url.port is not None and not 1 <= url.port <= 65535:
        raise ValueError(ERROR)
    _server_name(url.hostname)


def _read_regular(value, limit, *, private=False):
    path = Path(_text(value))
    if not path.is_absolute() or path.is_symlink() or path.resolve(strict=True) != path:
        raise ValueError(ERROR)
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode):
        raise ValueError(ERROR)
    flags = os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0) | getattr(os, 'O_BINARY', 0)
    descriptor = os.open(path, flags)
    with os.fdopen(descriptor, 'rb') as source:
        current = os.fstat(source.fileno())
        if not stat.S_ISREG(current.st_mode) or (before.st_dev, before.st_ino) != (current.st_dev, current.st_ino):
            raise ValueError(ERROR)
        if private and os.name == 'posix' and stat.S_IMODE(current.st_mode) & 0o077:
            raise ValueError(ERROR)
        if current.st_size > limit:
            raise ValueError(ERROR)
        contents = source.read(limit + 1)
        if len(contents) > limit:
            raise ValueError(ERROR)
        return contents


def _credential(value, seen):
    contents = _read_regular(value, 65, private=True).removesuffix(b'\n')
    if not re.fullmatch(b'[0-9a-f]{64}', contents) or contents in seen:
        raise ValueError(ERROR)
    seen.add(contents)
    return contents


def _new_output(output):
    if not isinstance(output, Path) or not output.is_absolute() or os.path.lexists(output):
        raise ValueError(ERROR)
    _text(str(output))
    if not output.parent.is_dir() or output.resolve(strict=False) != output:
        raise ValueError(ERROR)


def _validated(manifest, output):
    _object(manifest, ('prometheus', 'alertmanager', 'receiver', 'scrapes', 'rules_file'))
    if len(json.dumps(manifest, ensure_ascii=True, allow_nan=False).encode('utf-8')) > MANIFEST_LIMIT:
        raise ValueError(ERROR)
    _new_output(output)
    fields = ('listen', 'server_name', 'ca_file', 'cert_file', 'key_file', 'operator_password_file')
    prom = _object(manifest['prometheus'], fields)
    alert = _object(manifest['alertmanager'], (*fields, 'ingest_password_file'))
    receiver = _object(manifest['receiver'], ('url', 'ca_file', 'token_file'))
    if _socket(prom['listen']) == _socket(alert['listen']):
        raise ValueError(ERROR)
    secrets = set()
    passwords = {}
    for name, endpoint in (('prometheus', prom), ('alertmanager', alert)):
        _server_name(endpoint['server_name'])
        for kind in ('ca_file', 'cert_file', 'key_file'):
            _read_regular(endpoint[kind], MATERIAL_LIMIT, private=kind == 'key_file')
        passwords[name] = _credential(endpoint['operator_password_file'], secrets)
    passwords['ingest'] = _credential(alert['ingest_password_file'], secrets)
    _receiver_url(receiver['url'])
    _read_regular(receiver['ca_file'], MATERIAL_LIMIT)
    _credential(receiver['token_file'], secrets)
    scrapes = manifest['scrapes']
    if type(scrapes) is not list or not 1 <= len(scrapes) <= 4:
        raise ValueError(ERROR)
    jobs = set()
    for scrape in scrapes:
        _object(scrape, ('job', 'target', 'token_file'))
        job = _text(scrape['job'])
        if job not in JOBS or job in jobs:
            raise ValueError(ERROR)
        jobs.add(job)
        _socket(scrape['target'])
        _credential(scrape['token_file'], secrets)
    _read_regular(manifest['rules_file'], RULES_LIMIT)
    return passwords


def _hash(password):
    # Import only after manifest validation; a missing dependency also produces
    # the static CLI error and cannot leave partially generated policy files.
    import bcrypt
    return bcrypt.hashpw(password, bcrypt.gensalt(rounds=12)).decode('ascii')


def _configs(manifest, passwords):
    alert = manifest['alertmanager']
    receiver = manifest['receiver']
    prometheus = {
        'global': {'scrape_interval': '15s', 'evaluation_interval': '15s'},
        'rule_files': [manifest['rules_file']],
        'alerting': {'alertmanagers': [{
            'scheme': 'https',
            'static_configs': [{'targets': [alert['listen']]}],
            'basic_auth': {'username': 'prometheus', 'password_file': alert['ingest_password_file']},
            'tls_config': {'ca_file': alert['ca_file'], 'server_name': alert['server_name'], 'min_version': 'TLS13'},
            'follow_redirects': False,
        }]},
        'scrape_configs': [{
            'job_name': scrape['job'],
            'static_configs': [{'targets': [scrape['target']]}],
            'authorization': {'type': 'Bearer', 'credentials_file': scrape['token_file']},
            'follow_redirects': False,
        } for scrape in manifest['scrapes']],
    }
    alertmanager = {
        'route': {'receiver': 'local-operator-webhook',
                  'group_by': ['alertname', 'job', 'instance', 'listener', 'pool'],
                  'group_wait': '30s', 'group_interval': '5m', 'repeat_interval': '4h'},
        'receivers': [{'name': 'local-operator-webhook', 'webhook_configs': [{
            'url': receiver['url'], 'send_resolved': True,
            'http_config': {
                'authorization': {'type': 'Bearer', 'credentials_file': receiver['token_file']},
                'tls_config': {'ca_file': receiver['ca_file'], 'min_version': 'TLS13'},
                'follow_redirects': False,
            },
        }]}],
    }
    result = {'prometheus': prometheus, 'alertmanager': alertmanager}
    for name in ('prometheus', 'alertmanager'):
        endpoint = manifest[name]
        users = {'operator': _hash(passwords[name])}
        if name == 'alertmanager':
            users['prometheus'] = _hash(passwords['ingest'])
        result[name + '_web'] = {
            'tls_server_config': {'cert_file': endpoint['cert_file'], 'key_file': endpoint['key_file'], 'min_version': 'TLS13'},
            'basic_auth_users': users,
        }
    return result


def render(manifest: dict, output: Path) -> dict[str, Path]:
    """Validate completely, then exclusively create four private JSON/YAML files.

    Input certificate/key syntax is subsequently checked by the native tools.
    All failures are ValueError with a static message; existing output is untouched.
    """
    created = False
    written = []
    identity = None
    try:
        passwords = _validated(manifest, output)
        configs = _configs(manifest, passwords)
        encoded = {name: (json.dumps(config, indent=2, allow_nan=False) + '\n').encode('utf-8') for name, config in configs.items()}
        output.mkdir(mode=0o700)
        created = True
        identity = output.lstat()
        output.chmod(0o700)
        paths = {}
        for name, contents in encoded.items():
            path = output / (name.replace('_', '-') + '.yml')
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_BINARY', 0), 0o600)
            written.append(path)
            with os.fdopen(descriptor, 'wb') as destination:
                if os.name == 'posix':
                    os.fchmod(destination.fileno(), 0o600)
                destination.write(contents)
            paths[name] = path
        return paths
    except BaseException as error:
        if created:
            try:
                current = output.lstat()
                if identity is not None and stat.S_ISDIR(current.st_mode) and (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino):
                    for path in written:
                        path.unlink(missing_ok=True)
                    output.rmdir()
            except OSError:
                pass # Never broaden cleanup beyond files created by this call.
        if not isinstance(error, Exception):
            raise
        raise ValueError(ERROR) from None


def _unique_object(pairs):
    value = {}
    for name, item in pairs:
        if name in value:
            raise ValueError(ERROR)
        value[name] = item
    return value


def _invalid_constant(_value):
    raise ValueError(ERROR)


class _Parser(argparse.ArgumentParser):
    def error(self, _message):
        raise ValueError(ERROR)


def main():
    try:
        parser = _Parser(description=__doc__)
        parser.add_argument('--manifest', required=True)
        parser.add_argument('--output', required=True)
        args = parser.parse_args()
        raw = _read_regular(args.manifest, MANIFEST_LIMIT)
        manifest = json.loads(raw.decode('utf-8'), object_pairs_hook=_unique_object,
                              parse_constant=_invalid_constant)
        render(manifest, Path(args.output))
        print('Authenticated monitoring profile rendered.')
        return 0
    except Exception:
        print(ERROR, file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
