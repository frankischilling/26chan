"""Actual bounded Linux resource observations and authenticated HTTPS alerts."""

import argparse
from contextlib import ExitStack
from datetime import datetime, timezone
import importlib.util
import json
import math
import os
from pathlib import Path
import queue
import re
import secrets
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.parse

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / 'authenticated'))
from resource_fixture import OwnedFixture, ENVIRONMENT, private_json
from support import Receiver, make_pki, request, write_policy

_spec = importlib.util.spec_from_file_location('resource_auth_helpers', HERE / 'authenticated/qualify.py')
auth = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(auth)

ALERT_NAMES = frozenset(('BoardResourceObserverUnavailable', 'BoardStorageBytesPressure',
                        'BoardStorageInodesPressure', 'BoardStorageReadOnly', 'BoardServiceMemoryPressure',
                        'BoardServiceTasksPressure', 'BoardServiceCpuThrottling', 'BoardServiceOomKills'))
PHASE_NAMES = frozenset(('healthy control', 'drain prior notifications', 'enable pressure', 'native pressure',
                        'firing notification', 'matching firing rule', 'disable pressure', 'native recovery',
                        'resolved notification', 'matching resolution', 'nonfiring rule'))
CPU_METRICS = ('board_service_cpu_usage_seconds_total', 'board_service_cpu_quota_cores',
               'board_service_cpu_periods_total', 'board_service_cpu_throttled_periods_total')


def phase(name, label, operation, on_failure=None):
    if name not in ALERT_NAMES or label not in PHASE_NAMES:
        raise ValueError('Unknown bounded resource phase')
    print('STAGE ' + name + ' ' + label, flush=True)
    try:
        return operation()
    except Exception:
        print('STAGE failed ' + name + ' ' + label, flush=True)
        if on_failure is not None:
            try:
                on_failure()
            except Exception:
                print('STAGE resource failure diagnostics unavailable', flush=True)
        raise


def cpu_snapshot(data):
    result = {}
    for name in ('board_resource_sample_success', *CPU_METRICS):
        try:
            result[name] = metric(data, name, *(() if name == 'board_resource_sample_success' else ('service', 'public')))
        except (AssertionError, ValueError):
            result[name] = None
    return result


def accelerated_rules(source):
    return source.replace('[5m]', '[30s]').replace('for: 2m', 'for: 4s').replace('for: 30s', 'for: 4s')


def metric(data, name, label=None, target=None):
    suffix = '' if label is None else '{' + label + '="' + target + '"}'
    found = re.findall(rb'^' + re.escape((name + suffix).encode('ascii')) + rb' ([^\r\n ]+)\r?$', data, re.MULTILINE)
    if len(found) != 1:
        raise AssertionError('Resource metric is missing, duplicated or outside the owned target')
    value = float(found[0])
    if not math.isfinite(value) or value < 0:
        raise AssertionError('Resource metric is not finite and nonnegative')
    return value


def assert_unavailable(status, data):
    if (status != 503 or metric(data, 'board_resource_sample_success') != 0
            or re.search(rb'^board_(?:storage|service)_', data, re.MULTILINE)):
        raise AssertionError('Unavailable resource sample retained data or ready status')


def parse_starts_at(value):
    """Parse a bounded aware RFC3339 timestamp without losing nanoseconds."""
    if type(value) is not str or not 20 <= len(value) <= 35:
        raise ValueError('Invalid resource notification start time')
    match = re.fullmatch(r'([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2})'
                         r'(?:\.([0-9]{1,9}))?(Z|[+-][0-9]{2}:[0-9]{2})', value)
    if not match:
        raise ValueError('Invalid resource notification start time')
    zone = match[3]
    if zone != 'Z' and (zone == '-00:00' or int(zone[1:3]) > 23 or int(zone[4:]) > 59):
        raise ValueError('Invalid resource notification timezone')
    moment = datetime.fromisoformat(match[1] + ('+00:00' if zone == 'Z' else zone))
    elapsed = moment - datetime(1970, 1, 1, tzinfo=timezone.utc)
    return (elapsed.days * 86400 + elapsed.seconds) * 1_000_000_000 + int((match[2] or '').ljust(9, '0'))


def notification_identity(alert):
    if (type(alert) is not dict or type(alert.get('fingerprint')) is not str or not alert['fingerprint']
            or type(alert.get('labels')) is not dict or not alert['labels']):
        raise ValueError('Invalid resource notification identity')
    return parse_starts_at(alert.get('startsAt')), alert['startsAt'], alert['fingerprint'], alert['labels']


def find_notification(alerts, labels, status, *, activation_ns, firing=None):
    if type(activation_ns) is not int or activation_ns < 0 or status not in ('firing', 'resolved'):
        raise ValueError('Invalid resource activation boundary')
    chosen = None
    if status == 'resolved':
        chosen = notification_identity(firing)
        if firing.get('status') != 'firing' or chosen[0] < activation_ns:
            raise ValueError('Resolution requires the current firing generation')
    for alert in alerts:
        if type(alert) is not dict:
            continue
        actual = alert.get('labels', {})
        if type(actual) is not dict:
            continue
        if actual.get('alertname') != labels['alertname'] or alert.get('status') != status:
            continue
        if any(actual.get(name) != value for name, value in labels.items()):
            raise AssertionError('Resource notification escaped its owned target')
        try:
            identity = notification_identity(alert)
        except ValueError:
            continue
        if identity[0] < activation_ns or (chosen is not None and identity != chosen):
            continue
        return alert
    return None


def assert_same_alert(firing, resolved):
    try:
        matches = notification_identity(firing) == notification_identity(resolved)
    except ValueError:
        raise AssertionError('Resource recovery has no valid notification identity') from None
    if not matches or firing.get('status') != 'firing' or resolved.get('status') != 'resolved':
        raise AssertionError('Resource recovery does not match its firing alert')


def alertmanager_clear(base_url, ca, basic, name, instance):
    if name not in ALERT_NAMES:
        raise ValueError('Unknown resource alert')
    # v0.34.0 getAlertsHandler excludes expired alerts natively. Include every
    # current category, with no receiver/suppression filter that could conceal
    # an older generation. unprocessed=true is explicit even though the pinned
    # handler currently includes pending entries without consulting that flag.
    parameters = [(key, 'true') for key in ('active', 'silenced', 'inhibited', 'unprocessed')]
    labels = {'alertname': name, 'job': 'board-resource', 'instance': instance}
    parameters.extend(('filter', key + '=' + json.dumps(value)) for key, value in labels.items())
    status, body = request(base_url.rstrip('/') + '/api/v2/alerts?' + urllib.parse.urlencode(parameters),
                           ca, basic=basic)
    if status != 200:
        raise AssertionError('Authenticated Alertmanager healthy boundary is unavailable')

    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('Duplicate response field')
            result[key] = value
        return result

    def invalid_constant(_value):
        raise ValueError('Invalid response constant')

    try:
        alerts = json.loads(body, object_pairs_hook=unique, parse_constant=invalid_constant)
        if type(alerts) is not list or len(alerts) > 256:
            raise ValueError('Invalid current alerts response')
        for alert in alerts:
            if type(alert) is not dict:
                raise ValueError('Invalid current alert')
            actual, state = alert.get('labels'), alert.get('status')
            if (type(actual) is not dict or not all(type(key) is str and type(value) is str for key, value in actual.items())
                    or any(actual.get(key) != value for key, value in labels.items())
                    or type(state) is not dict or state.get('state') not in ('active', 'suppressed', 'unprocessed')):
                raise ValueError('Invalid filtered current alert')
    except (ValueError, TypeError, RecursionError):
        raise AssertionError('Alertmanager healthy boundary returned an invalid response') from None
    return not alerts


def qualify(root, binary, binary_directory, lifecycle_state):
    fixture = OwnedFixture(root)
    children = []
    try:
        print('STAGE setup owned Linux resource fixture', flush=True)
        fixture.setup(binary)
        with tempfile.TemporaryDirectory(prefix='stack-', dir=fixture.private) as directory, ExitStack() as cleanup:
            work = Path(directory).resolve()
            print('STAGE create owned synthetic monitoring PKI', flush=True)
            pki = make_pki(work / 'pki', '/usr/bin/openssl')
            credentials = {name: secrets.token_hex(32) for name in
                           ('prom-operator', 'alert-operator', 'alert-ingest', 'receiver', 'scrape')}
            paths = {}
            for name, value in credentials.items():
                path = work / (name + '.token')
                descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                with os.fdopen(descriptor, 'w') as output:
                    output.write(value)
                paths[name] = path
            policy = work / 'receiver.json'
            write_policy(policy, credentials['receiver'])
            receiver = cleanup.enter_context(Receiver(pki['cert'], pki['key'], policy))
            metrics_port, prom_port, alert_port = (auth.port() for _ in range(3))
            if len({metrics_port, prom_port, alert_port, receiver.port}) != 4:
                raise AssertionError('Owned resource qualification port collision')
            address = '127.0.0.1:' + str(metrics_port)
            print('STAGE start distinct observer and both authority probes', flush=True)
            fixture.start_observer(address, credentials['scrape'])
            metrics_url = 'http://' + address

            def scrape():
                status, data = auth.plain_request(metrics_url + '/metrics', credentials['scrape'])
                if status != 200:
                    raise AssertionError('Authenticated resource scrape failed')
                if str(root).encode() in data or b'protected-payload' in data or credentials['scrape'].encode() in data:
                    raise AssertionError('Resource metrics exposed private source information')
                return data

            auth.wait_for('healthy native resource sample',
                          lambda: metric(scrape(), 'board_resource_sample_success') == 1, children, 25)
            for token in (None, '0' * 64):
                if auth.plain_request(metrics_url + '/metrics', token)[0] != 401:
                    raise AssertionError('Resource endpoint accepted missing/wrong credential')
            if auth.plain_request(metrics_url + '/readyz', credentials['scrape'])[0] != 200:
                raise AssertionError('Healthy native resource sample is not ready')
            data = scrape()
            if (metric(data, 'board_storage_capacity_bytes', 'storage', 'database') != 16 * 1024 * 1024
                    or metric(data, 'board_storage_inodes', 'storage', 'database') != 256
                    or metric(data, 'board_storage_read_only', 'storage', 'database') != 0
                    or metric(data, 'board_service_memory_limit_bytes', 'service', 'public') != 64 * 1024 * 1024
                    or metric(data, 'board_service_tasks_limit', 'service', 'public') != 16
                    or metric(data, 'board_service_cpu_quota_cores', 'service', 'public') != 0.2):
                raise AssertionError('Real resource measurements do not match the bounded native fixture')
            proof = fixture.private / (fixture.observer_unit + '.log')
            if proof.read_text().count('PASS distinct observer reads statistics and denies payload/control authority') != 2:
                raise AssertionError('Observer authority denial lacks both pre-exec and post-healthy evidence')
            positive = fixture.source / 'root-write-control'
            positive.write_bytes(b'owned positive write control')
            if positive.read_bytes() != b'owned positive write control':
                raise AssertionError('Root-owned storage write control failed')
            positive.unlink()
            protected = fixture.source / 'protected-payload'
            protected.write_bytes(protected.read_bytes())
            print('PASS real Linux finite storage/cgroup metrics, denied scrapes and distinct observer authority', flush=True)

            common = {'server_name': 'localhost', 'ca_file': str(pki['ca']),
                      'cert_file': str(pki['cert']), 'key_file': str(pki['key'])}
            manifest = {
                'prometheus': dict(common, listen='127.0.0.1:' + str(prom_port),
                                   operator_password_file=str(paths['prom-operator'])),
                'alertmanager': dict(common, listen='127.0.0.1:' + str(alert_port),
                                     operator_password_file=str(paths['alert-operator']),
                                     ingest_password_file=str(paths['alert-ingest'])),
                'receiver': {'url': receiver.url, 'ca_file': str(pki['ca']), 'token_file': str(paths['receiver'])},
                'scrapes': [{'job': 'board-resource', 'target': address, 'token_file': str(paths['scrape'])}],
                'rules_file': str((ROOT / 'deploy/monitoring/alerts.yml').resolve()),
            }
            print('STAGE render private authenticated resource profile', flush=True)
            configs = auth.render(manifest, work / 'profile')
            tools = {name: binary_directory / name for name in ('prometheus', 'promtool', 'alertmanager', 'amtool')}
            print('STAGE validate native HTTPS configuration', flush=True)
            for command in ([tools['promtool'], 'check', 'config', configs['prometheus']],
                            [tools['promtool'], 'check', 'web-config', configs['prometheus_web'], configs['alertmanager_web']],
                            [tools['amtool'], 'check-config', configs['alertmanager']]):
                result = subprocess.run(command, capture_output=True, timeout=20, env=ENVIRONMENT)
                if result.returncode:
                    raise AssertionError('Native generated resource HTTPS configuration failed validation')
            rules = work / 'alerts.yml'
            rules.write_text(accelerated_rules(Path(manifest['rules_file']).read_text()))
            prometheus = json.loads(configs['prometheus'].read_text())
            prometheus['global'].update(scrape_interval='1s', evaluation_interval='1s')
            prometheus['rule_files'] = [str(rules)]
            private_json(configs['prometheus'], prometheus)
            alertmanager = json.loads(configs['alertmanager'].read_text())
            alertmanager['route'].update(group_wait='1s', group_interval='1s')
            private_json(configs['alertmanager'], alertmanager)

            def launch(name, command):
                log = cleanup.enter_context((work / (name + '.log')).open('wb'))
                process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, env=ENVIRONMENT)
                cleanup.callback(auth.stop, process)
                children.append((name, process))

            print('STAGE start authenticated native monitoring stack', flush=True)
            launch('alertmanager', [tools['alertmanager'], '--config.file=' + str(configs['alertmanager']),
                   '--web.config.file=' + str(configs['alertmanager_web']), '--storage.path=' + str(work / 'alert-data'),
                   '--web.listen-address=127.0.0.1:' + str(alert_port), '--cluster.listen-address=', '--log.level=warn'])
            launch('prometheus', [tools['prometheus'], '--config.file=' + str(configs['prometheus']),
                   '--web.config.file=' + str(configs['prometheus_web']), '--storage.tsdb.path=' + str(work / 'prom-data'),
                   '--storage.tsdb.retention.time=1h', '--storage.tsdb.retention.size=32MB',
                   '--web.listen-address=127.0.0.1:' + str(prom_port), '--log.level=warn'])
            prom_url = 'https://localhost:' + str(prom_port)
            prom_basic = ('operator', credentials['prom-operator'])
            am_url = 'https://localhost:' + str(alert_port)
            am_basic = ('operator', credentials['alert-operator'])

            def query(expression):
                status, body = request(prom_url + '/api/v1/query?' + urllib.parse.urlencode({'query': expression}),
                                       pki['ca'], basic=prom_basic)
                if status != 200:
                    raise AssertionError('Authenticated resource Prometheus query failed')
                parsed = json.loads(body)
                if parsed['status'] != 'success':
                    raise AssertionError('Resource query evaluation failed')
                return parsed['data']['result']

            auth.wait_for('actual authenticated resource scrape', lambda: query('up{job="board-resource"} == 1'), children, 30)
            for url, basic in ((prom_url + '/api/v1/status/runtimeinfo', prom_basic),
                               (am_url + '/api/v2/status', am_basic)):
                auth.wait_for('native authenticated API', lambda: request(url, pki['ca'], basic=basic)[0] == 200, children, 20)
                for bad in (None, (basic[0], '0' * 64)):
                    if request(url, pki['ca'], basic=bad)[0] != 401:
                        raise AssertionError('Resource qualification native API accepted a bad credential')
            if query('ALERTS{job="board-resource",alertstate="firing"}'):
                raise AssertionError('Healthy resource fixture unexpectedly alerts')
            private_json(lifecycle_state, {'work': str(work), 'children': [child.pid for _, child in children],
                                          'ports': [metrics_port, prom_port, alert_port, receiver.port],
                                          'units': [fixture.fixture_unit, fixture.observer_unit],
                                          'mount': str(fixture.mount)})
            print('PASS actual resource scrape through the generated authenticated HTTPS profile', flush=True)
            seen = []

            def drain_notifications():
                while True:
                    try:
                        seen.extend(receiver.notifications.get_nowait().get('alerts', []))
                    except queue.Empty:
                        break

            def notification(name, status, dimension, target, *, activation_ns, firing=None):
                drain_notifications()
                labels = {'alertname': name, 'job': 'board-resource', 'instance': address}
                if dimension:
                    labels[dimension] = target
                return find_notification(seen, labels, status, activation_ns=activation_ns, firing=firing)

            def healthy_control(name):
                return (not query('ALERTS{job="board-resource",alertname="' + name + '"}')
                        and alertmanager_clear(am_url, pki['ca'], am_basic, name, address))

            def alert_cycle(name, enable, disable, native_high, native_low, dimension, target):
                print('STAGE ' + name + ' bounded native pressure', flush=True)
                evidence = {}

                def diagnose_cpu():
                    # Only fixed metric names and validated scalars are emitted.
                    # Never print a response body, label, endpoint or credential.
                    try:
                        for metric_name, value in cpu_snapshot(scrape()).items():
                            print('STAGE CPU metric ' + metric_name + ' ' + (str(value) if value is not None else 'unavailable'), flush=True)
                    except Exception:
                        print('STAGE CPU metrics unavailable', flush=True)
                    for metric_name in CPU_METRICS[2:]:
                        try:
                            samples = query('rate(' + metric_name + '{job="board-resource",service="public",instance="'
                                            + address + '"}[30s])')
                            if len(samples) != 1:
                                raise ValueError('Unavailable rate')
                            value = float(samples[0]['value'][1])
                            if not math.isfinite(value) or value < 0:
                                raise ValueError('Invalid rate')
                            print('STAGE CPU rate ' + metric_name + ' ' + str(value), flush=True)
                        except Exception:
                            print('STAGE CPU rate ' + metric_name + ' unavailable', flush=True)
                    if 'firing' in evidence and 'resolved' in evidence:
                        for field in ('fingerprint', 'labels', 'startsAt'):
                            if evidence['firing'].get(field) != evidence['resolved'].get(field):
                                print('STAGE resource notification mismatch ' + field, flush=True)

                def step(label, operation):
                    return phase(name, label, operation, diagnose_cpu if name == 'BoardServiceCpuThrottling' else None)

                step('healthy control', lambda: auth.wait_for('healthy control before ' + name,
                     lambda: healthy_control(name), children, 55))
                step('drain prior notifications', drain_notifications)
                seen[:] = [alert for alert in seen if alert.get('labels', {}).get('alertname') != name]
                # Prometheus and this driver use the same trusted host UTC
                # clock. Even a cleared Prometheus/Alertmanager generation is
                # not a webhook barrier: prior messages can outlive the drain.
                activation_ns = time.time_ns()
                step('enable pressure', enable)
                try:
                    step('native pressure', lambda: auth.wait_for('native ' + name + ' pressure', lambda: native_high(scrape()), children, 25))
                    firing = step('firing notification', lambda: auth.wait_for(name + ' firing HTTPS notification',
                                  lambda: notification(name, 'firing', dimension, target, activation_ns=activation_ns), children, 55))
                    evidence['firing'] = firing

                    def verify_firing():
                        if not query('ALERTS{job="board-resource",alertname="' + name + '",alertstate="firing"}'):
                            raise AssertionError('Received resource notification has no live firing rule')

                    step('matching firing rule', verify_firing)
                finally:
                    step('disable pressure', disable)
                step('native recovery', lambda: auth.wait_for('native ' + name + ' recovery', lambda: native_low(scrape()), children, 25))
                resolved = step('resolved notification', lambda: auth.wait_for(name + ' resolved HTTPS notification',
                                lambda: notification(name, 'resolved', dimension, target,
                                                     activation_ns=activation_ns, firing=firing), children, 65))
                evidence['resolved'] = resolved
                step('matching resolution', lambda: assert_same_alert(firing, resolved))
                step('nonfiring rule', lambda: auth.wait_for('nonfiring ' + name,
                     lambda: not query('ALERTS{job="board-resource",alertname="' + name + '",alertstate="firing"}'), children, 15))
                print('PASS ' + name + ' actual pressure and matching firing/resolved HTTPS delivery', flush=True)

            def ratio(data, numerator, denominator, dimension, target):
                return metric(data, numerator, dimension, target) / metric(data, denominator, dimension, target)

            for name, control, numerator, denominator, dimension, target, low_is_pressure in (
                    ('BoardStorageBytesPressure', fixture.bytes_pressure, 'board_storage_available_bytes',
                     'board_storage_capacity_bytes', 'storage', 'database', True),
                    ('BoardStorageInodesPressure', fixture.inode_pressure, 'board_storage_available_inodes',
                     'board_storage_inodes', 'storage', 'database', True),
                    ('BoardServiceMemoryPressure', lambda on: fixture.command('memory_on' if on else 'memory_off'),
                     'board_service_memory_bytes', 'board_service_memory_limit_bytes', 'service', 'public', False),
                    ('BoardServiceTasksPressure', lambda on: fixture.command('tasks_on' if on else 'tasks_off'),
                     'board_service_tasks', 'board_service_tasks_limit', 'service', 'public', False)):
                def high(data):
                    value = ratio(data, numerator, denominator, dimension, target)
                    return value < 0.1 if low_is_pressure else value > 0.9

                def low(data):
                    value = ratio(data, numerator, denominator, dimension, target)
                    return value > 0.5 if low_is_pressure else value < 0.8

                alert_cycle(name, lambda: control(True), lambda: control(False), high, low, dimension, target)
            initial = scrape()
            initial_cpu = metric(initial, 'board_service_cpu_usage_seconds_total', 'service', 'public')
            initial_throttled = metric(initial, 'board_service_cpu_throttled_periods_total', 'service', 'public')
            alert_cycle('BoardServiceCpuThrottling', lambda: fixture.command('cpu_on'), lambda: fixture.command('cpu_off'),
                        lambda data: metric(data, 'board_service_cpu_usage_seconds_total', 'service', 'public') > initial_cpu
                        and metric(data, 'board_service_cpu_throttled_periods_total', 'service', 'public') > initial_throttled,
                        lambda data: metric(data, 'board_resource_sample_success') == 1, 'service', 'public')
            # Read-only is an actual tmpfs remount, not an invented numeric input.
            alert_cycle('BoardStorageReadOnly', lambda: fixture.readonly(True), lambda: fixture.readonly(False),
                        lambda data: metric(data, 'board_storage_read_only', 'storage', 'database') == 1,
                        lambda data: metric(data, 'board_storage_read_only', 'storage', 'database') == 0, 'storage', 'database')
            before_oom = metric(scrape(), 'board_service_memory_oom_kills_total', 'service', 'public')
            alert_cycle('BoardServiceOomKills', lambda: fixture.command('oom'), lambda: None,
                        lambda data: metric(data, 'board_service_memory_oom_kills_total', 'service', 'public') > before_oom,
                        lambda data: metric(data, 'board_resource_sample_success') == 1, 'service', 'public')

            # Deny traversal through a parent of the configured source. O_PATH
            # may open a mode000 target itself, so denying only the target would
            # not establish actual source unavailability.
            unavailable_name = 'BoardResourceObserverUnavailable'

            def unavailable_healthy_control():
                return metric(scrape(), 'board_resource_sample_success') == 1 and healthy_control(unavailable_name)

            phase(unavailable_name, 'healthy control', lambda: auth.wait_for('healthy control before source unavailability',
                  unavailable_healthy_control, children, 55))
            phase(unavailable_name, 'drain prior notifications', drain_notifications)
            seen[:] = [alert for alert in seen if alert.get('labels', {}).get('alertname') != unavailable_name]
            unavailable_activation_ns = time.time_ns()
            phase(unavailable_name, 'enable pressure', lambda: fixture.mount.chmod(0o000))
            try:
                def unavailable():
                    sample = scrape()
                    if metric(sample, 'board_resource_sample_success') != 0:
                        return False
                    assert_unavailable(auth.plain_request(metrics_url + '/readyz', credentials['scrape'])[0], sample)
                    return auth.plain_request(metrics_url + '/healthz', credentials['scrape'])[0] == 200

                phase(unavailable_name, 'native pressure',
                      lambda: auth.wait_for('native unavailable source and readiness503', unavailable, children, 20))
                firing = phase(unavailable_name, 'firing notification', lambda: auth.wait_for('unavailable resource HTTPS notification',
                               lambda: notification(unavailable_name, 'firing', None, None,
                                                    activation_ns=unavailable_activation_ns), children, 40))
            finally:
                phase(unavailable_name, 'disable pressure', lambda: fixture.mount.chmod(0o711))
            phase(unavailable_name, 'native recovery', lambda: auth.wait_for('restored actual source',
                  lambda: metric(scrape(), 'board_resource_sample_success') == 1, children, 20))
            resolved = phase(unavailable_name, 'resolved notification', lambda: auth.wait_for('unavailable resource resolved HTTPS notification',
                             lambda: notification(unavailable_name, 'resolved', None, None,
                                                  activation_ns=unavailable_activation_ns, firing=firing), children, 40))
            phase(unavailable_name, 'matching resolution', lambda: assert_same_alert(firing, resolved))
            print('PASS actual unavailable source omits data, readyz503/healthz200 and authenticated recovery', flush=True)
            fixture.stop_unit(fixture.observer_unit, require_success=True)
            state = fixture.unit_info(fixture.observer_unit)
            if state.get('ActiveState') not in ('inactive', None):
                raise AssertionError('Resource observer did not stop normally')
    finally:
        fixture.cleanup()


if __name__ == '__main__':
    auth.install_signal_cleanup()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--tools', type=Path, required=True)
    args = parser.parse_args()
    try:
        qualify(args.root, args.binary, args.tools, args.root / 'state.json')
    except Exception:
        print('Owned resource qualification failed; no private inputs are logged.', file=sys.stderr, flush=True)
        raise SystemExit(1) from None
