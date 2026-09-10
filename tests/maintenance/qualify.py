"""Actual operator outcomes through a distinct observer and authenticated alerts."""

import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import queue
import secrets
import subprocess
import sys
import tempfile
import time
import urllib.parse

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / 'tests/monitoring'))
from resource_qualify import auth, find_notification, assert_same_alert, metric
from resource_qualify import Receiver, make_pki, request, write_policy
from fixture import ENVIRONMENT, OwnedFixture, private_json

NAMES = frozenset(('BoardMaintenanceFailed', 'BoardMaintenanceOverdue', 'BoardMaintenanceObserverUnavailable'))


def current_alerts_clear(url, ca, basic, labels):
    if labels.get('alertname') not in NAMES or labels.get('job') != 'board-maintenance':
        raise ValueError('Unknown maintenance alert boundary')
    parameters = [(name, 'true') for name in ('active', 'silenced', 'inhibited', 'unprocessed')]
    parameters += [('filter', name + '=' + json.dumps(value)) for name, value in labels.items()]
    status, body = request(url + '/api/v2/alerts?' + urllib.parse.urlencode(parameters), ca, basic=basic)
    if status != 200:
        raise AssertionError('Authenticated maintenance alert boundary unavailable')

    def unique(pairs):
        result = {}
        for name, value in pairs:
            if name in result:
                raise ValueError('Duplicate alert response field')
            result[name] = value
        return result

    try:
        alerts = json.loads(body, object_pairs_hook=unique)
        if type(alerts) is not list or len(alerts) > 256:
            raise ValueError('Invalid alert list')
        for alert in alerts:
            if (type(alert) is not dict or type(alert.get('labels')) is not dict
                    or any(alert['labels'].get(name) != value for name, value in labels.items())
                    or type(alert.get('status')) is not dict
                    or alert['status'].get('state') not in ('active', 'suppressed', 'unprocessed')):
                raise ValueError('Invalid current maintenance alert')
    except (ValueError, TypeError, RecursionError):
        raise AssertionError('Invalid maintenance alert boundary response') from None
    return not alerts


def qualify(root, binary, binary_directory):
    fixture, children = OwnedFixture(root), []
    try:
        print('STAGE create owned real maintenance update and journal', flush=True)
        fixture.setup(binary)
        with tempfile.TemporaryDirectory(prefix='stack-', dir=fixture.private) as directory, ExitStack() as cleanup:
            work = Path(directory).resolve()
            pki = make_pki(work / 'pki', '/usr/bin/openssl')
            credentials = {name: secrets.token_hex(32) for name in ('prom-operator', 'alert-operator', 'alert-ingest', 'receiver', 'scrape')}
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
                raise AssertionError('Maintenance qualification port collision')
            address = '127.0.0.1:' + str(metrics_port)
            metrics_url = 'http://' + address
            print('STAGE start distinct production-mode maintenance observer', flush=True)
            fixture.start_observer(address, credentials['scrape'])

            def scrape():
                status, data = auth.plain_request(metrics_url + '/metrics', credentials['scrape'])
                if status != 200:
                    raise AssertionError('Authenticated maintenance scrape failed')
                if str(root).encode() in data or credentials['scrape'].encode() in data or b'version1' in data:
                    raise AssertionError('Maintenance metrics exposed private inputs')
                return data

            def value(name):
                return metric(scrape(), 'board_maintenance_' + name, 'maintenance', 'application')

            auth.wait_for('actual healthy maintenance sample', lambda: value('sample_success') == 1, children, 25)
            for token in (None, '0' * 64):
                if auth.plain_request(metrics_url + '/metrics', token)[0] != 401:
                    raise AssertionError('Maintenance scrape accepted a bad credential')
            if auth.plain_request(metrics_url + '/readyz', credentials['scrape'])[0] != 200:
                raise AssertionError('Healthy maintenance observer not ready')
            if (fixture.private / 'observer.log').read_text().count('PASS distinct observer reads journal') != 2:
                raise AssertionError('Missing pre-exec and post-healthy authority proof')
            print('PASS real update journal, distinct observer authority and denied scrapes', flush=True)
            common = {'server_name': 'localhost', 'ca_file': str(pki['ca']), 'cert_file': str(pki['cert']), 'key_file': str(pki['key'])}
            manifest = {
                'prometheus': dict(common, listen='127.0.0.1:' + str(prom_port), operator_password_file=str(paths['prom-operator'])),
                'alertmanager': dict(common, listen='127.0.0.1:' + str(alert_port),
                                     operator_password_file=str(paths['alert-operator']), ingest_password_file=str(paths['alert-ingest'])),
                'receiver': {'url': receiver.url, 'ca_file': str(pki['ca']), 'token_file': str(paths['receiver'])},
                'scrapes': [{'job': 'board-maintenance', 'target': address, 'token_file': str(paths['scrape'])}],
                'rules_file': str(ROOT / 'deploy/monitoring/alerts.yml'),
            }
            configs = auth.render(manifest, work / 'profile')
            tools = {name: binary_directory / name for name in ('prometheus', 'promtool', 'alertmanager', 'amtool')}
            for command in ([tools['promtool'], 'check', 'config', configs['prometheus']],
                            [tools['promtool'], 'check', 'web-config', configs['prometheus_web'], configs['alertmanager_web']],
                            [tools['amtool'], 'check-config', configs['alertmanager']]):
                result = subprocess.run(command, capture_output=True, timeout=20, env=ENVIRONMENT)
                if result.returncode:
                    raise AssertionError('Native maintenance monitoring configuration rejected')
            rules = work / 'alerts.yml'
            rules.write_text(Path(manifest['rules_file']).read_text().replace('for: 30s', 'for: 3s'))
            prom = json.loads(configs['prometheus'].read_text())
            prom['global'].update(scrape_interval='1s', evaluation_interval='1s')
            prom['rule_files'] = [str(rules)]
            private_json(configs['prometheus'], prom)
            am = json.loads(configs['alertmanager'].read_text())
            am['route'].update(group_wait='1s', group_interval='1s')
            private_json(configs['alertmanager'], am)

            def launch(name, command):
                log = cleanup.enter_context((work / (name + '.log')).open('wb'))
                child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, env=ENVIRONMENT)
                cleanup.callback(auth.stop, child)
                children.append((name, child))

            launch('alertmanager', [tools['alertmanager'], '--config.file=' + str(configs['alertmanager']),
                   '--web.config.file=' + str(configs['alertmanager_web']), '--storage.path=' + str(work / 'alert-data'),
                   '--web.listen-address=127.0.0.1:' + str(alert_port), '--cluster.listen-address=', '--log.level=warn'])
            launch('prometheus', [tools['prometheus'], '--config.file=' + str(configs['prometheus']),
                   '--web.config.file=' + str(configs['prometheus_web']), '--storage.tsdb.path=' + str(work / 'prom-data'),
                   '--storage.tsdb.retention.time=1h', '--storage.tsdb.retention.size=32MB',
                   '--web.listen-address=127.0.0.1:' + str(prom_port), '--log.level=warn'])
            prom_url, am_url = 'https://localhost:' + str(prom_port), 'https://localhost:' + str(alert_port)
            prom_basic, am_basic = ('operator', credentials['prom-operator']), ('operator', credentials['alert-operator'])

            def query(expression):
                status, body = request(prom_url + '/api/v1/query?' + urllib.parse.urlencode({'query': expression}), pki['ca'], basic=prom_basic)
                if status != 200:
                    raise AssertionError('Maintenance Prometheus API unavailable')
                parsed = json.loads(body)
                if parsed.get('status') != 'success' or type(parsed.get('data', {}).get('result')) is not list:
                    raise AssertionError('Maintenance query evaluation failed')
                return parsed['data']['result']

            auth.wait_for('authenticated actual maintenance scrape', lambda: query('up{job="board-maintenance"} == 1'), children, 30)
            for url, basic in ((prom_url + '/api/v1/status/runtimeinfo', prom_basic), (am_url + '/api/v2/status', am_basic)):
                auth.wait_for('maintenance native API', lambda: request(url, pki['ca'], basic=basic)[0] == 200, children, 20)
                for bad in (None, ('operator', '0' * 64)):
                    if request(url, pki['ca'], basic=bad)[0] != 401:
                        raise AssertionError('Maintenance native API accepted bad credentials')
            private_json(root / 'state.json', {'work': str(work), 'children': [child.pid for _, child in children],
                         'ports': [metrics_port, prom_port, alert_port, receiver.port],
                         'units': [fixture.observer_unit, fixture.producer_unit]})
            print('PASS actual maintenance scrape through generated authenticated HTTPS profile', flush=True)
            seen = []

            def drain():
                while True:
                    try:
                        seen.extend(receiver.notifications.get_nowait()['alerts'])
                    except queue.Empty:
                        return

            def cycle(name, enable, restore, native_high):
                if name not in NAMES:
                    raise ValueError('Unknown maintenance cycle')
                labels = {'alertname': name, 'job': 'board-maintenance', 'instance': address, 'maintenance': 'application'}
                selector = 'ALERTS{job="board-maintenance",alertname="' + name + '"}'
                print('STAGE ' + name + ' healthy control', flush=True)
                auth.wait_for('healthy maintenance control', lambda: value('sample_success') == 1 and not query(selector)
                              and current_alerts_clear(am_url, pki['ca'], am_basic, labels), children, 55)
                drain()
                boundary = time.time_ns()
                print('STAGE ' + name + ' actual condition', flush=True)
                enable()
                try:
                    auth.wait_for('native maintenance condition', native_high, children, 45)
                    print('STAGE ' + name + ' firing delivery', flush=True)

                    def notification(status, firing=None):
                        drain()
                        return find_notification(seen, labels, status, activation_ns=boundary, firing=firing)

                    firing = auth.wait_for('current maintenance firing', lambda: notification('firing'), children, 55)
                    if not query(selector[:-1] + ',alertstate="firing"}'):
                        raise AssertionError('Maintenance notification lacks live firing rule')
                finally:
                    restore()
                print('STAGE ' + name + ' recovery delivery', flush=True)
                auth.wait_for('healthy journal observation after restoration', lambda: value('sample_success') == 1, children, 25)
                resolved = auth.wait_for('matching maintenance resolution', lambda: notification('resolved', firing), children, 55)
                assert_same_alert(firing, resolved)
                auth.wait_for('maintenance nonfiring rule', lambda: not query(selector), children, 20)
                print('PASS ' + name + ' actual condition and matching current firing/resolved HTTPS delivery', flush=True)

            def fail_update():
                fixture.source.unlink()
                fixture.produce(failure=True)
                if fixture.installed.read_bytes() != b'version1':
                    raise AssertionError('Failed owned update changed installed marker')

            def recover_update(version):
                fixture.source.write_bytes(version)
                fixture.produce()

            cycle('BoardMaintenanceFailed', fail_update, lambda: recover_update(b'version2'), lambda: value('failure_pending') == 1)
            recover_update(b'version2')
            cycle('BoardMaintenanceOverdue', lambda: None, lambda: recover_update(b'version3'),
                  lambda: time.time() - value('last_success_timestamp_seconds') > value('max_age_seconds'))

            def unavailable():
                if value('sample_success') != 0:
                    return False
                sample = scrape()
                if any(('board_maintenance_' + name + '{').encode() in sample for name in
                       ('failure_pending', 'run_in_progress', 'run_started_timestamp_seconds', 'last_success_timestamp_seconds', 'max_age_seconds', 'run_timeout_seconds')):
                    raise AssertionError('Unavailable maintenance target retained outcome data')
                return (auth.plain_request(metrics_url + '/readyz', credentials['scrape'])[0] == 503
                        and auth.plain_request(metrics_url + '/healthz', credentials['scrape'])[0] == 200)

            cycle('BoardMaintenanceObserverUnavailable', lambda: fixture.states.chmod(0o700),
                  lambda: fixture.states.chmod(0o755), unavailable)
            journal_path = fixture.states / 'application.json'
            retained_path = fixture.states / 'application.saved'
            cycle('BoardMaintenanceObserverUnavailable', lambda: journal_path.rename(retained_path),
                  lambda: retained_path.rename(journal_path), unavailable)
            fixture.stop_unit(fixture.observer_unit, require_success=True)
    finally:
        fixture.cleanup()


if __name__ == '__main__':
    auth.install_signal_cleanup()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--tools', type=Path, required=True)
    arguments = parser.parse_args()
    try:
        qualify(arguments.root, arguments.binary, arguments.tools)
    except Exception:
        print('Owned maintenance qualification failed; private inputs are not logged.', file=sys.stderr)
        raise SystemExit(1) from None
