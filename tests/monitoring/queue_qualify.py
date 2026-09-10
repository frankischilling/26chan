"""Real disposable queue -> monitor -> Prometheus -> Alertmanager qualification."""

from contextlib import ExitStack
import argparse
import http.server
import json
import os
from pathlib import Path
import queue
import re
import secrets
import subprocess
import tempfile
import threading
import urllib.parse

from qualify import ROOT, install_signal_cleanup, port, request, stop, wait_for


QUEUE_FAMILIES = {
    'board_media_queue_capacity', 'board_media_jobs', 'board_media_expired_jobs',
    'board_media_oldest_queued_seconds', 'board_media_failures_recent',
}


def accelerated_rules(source):
    # Change alert hold times only. Expressions, thresholds, freshness checks,
    # rate windows and the database's production 15-minute window stay intact.
    return re.sub(r'(?m)^(\s*for:) (?:30s|1m)$', r'\1 4s', source)


def sample_values(exposition):
    values = {}
    for line in exposition.decode('utf-8').splitlines():
        if line and not line.startswith('#'):
            key, value = line.rsplit(' ', 1)
            if key in values:
                raise AssertionError('Duplicate exposition series')
            values[key] = float(value)
    return values


def assert_unavailable(readiness_status, exposition):
    values = sample_values(exposition)
    if readiness_status != 503 or values.get('board_media_sample_success') != 0:
        raise AssertionError('Failed sampling did not become unavailable')
    if values.get('board_media_sample_last_success_timestamp_seconds', 0) <= 0:
        raise AssertionError('Last successful sample timestamp was lost')
    if any(key.split('{', 1)[0] in QUEUE_FAMILIES for key in values):
        raise AssertionError('Unavailable observer retained queue values')


def find_notification(seen, name, status, instance):
    for alert in seen:
        labels = alert.get('labels', {})
        if labels.get('alertname') == name and alert.get('status') == status:
            if labels.get('job') != 'board-monitor' or labels.get('instance') != instance:
                raise AssertionError('Alert evidence came from a different target')
            if not alert.get('fingerprint'):
                raise AssertionError('Alert evidence has no fingerprint')
            return alert
    return None


def assert_same_alert(firing, resolved):
    if (firing.get('status') != 'firing' or resolved.get('status') != 'resolved'
            or firing.get('fingerprint') != resolved.get('fingerprint')
            or firing.get('labels') != resolved.get('labels')):
        raise AssertionError('Resolved notification does not identify the firing alert')


def write_lifecycle_state(destination, work, child_pids):
    # Publish only after actual sampling/scraping works. Atomic replacement keeps
    # the interruption watcher from reading a partially written document.
    state = {'directory': str(work), 'children': child_pids}
    temporary = destination.with_suffix('.tmp')
    temporary.write_text(json.dumps(state), encoding='utf-8')
    temporary.replace(destination)


def qualify(lifecycle_state=None):
    if os.environ.get('QUEUE_QUALIFICATION') != 'owned-disposable':
        raise AssertionError('Use the owned disposable PostgreSQL helper')
    system_keys = {'systemroot', 'windir', 'path', 'temp', 'tmp', 'tmpdir'}
    minimal = {key: value for key, value in os.environ.items() if key.lower() in system_keys}
    monitor = Path(os.environ['BOARD_MONITOR_BIN']).resolve()
    fixture = Path(os.environ['QUEUE_FIXTURE_BIN']).resolve()
    directory = Path(os.environ['MONITORING_BIN_DIR']).resolve()
    suffix = '.exe' if os.name == 'nt' else ''
    tools = {name: directory / (name + suffix) for name in ('prometheus', 'promtool', 'alertmanager', 'amtool')}
    if not all(binary.is_file() for binary in (monitor, fixture, *tools.values())):
        raise AssertionError('A required qualification binary is absent')
    fixture_environment = dict(minimal, QUEUE_QUALIFICATION='owned-disposable',
                               MEDIA_DATABASE_URL=os.environ['MEDIA_DATABASE_URL'],
                               MIGRATION_DATABASE_URL=os.environ['MIGRATION_DATABASE_URL'])
    fixture_tag = secrets.token_hex(16)

    def transition(command):
        result = subprocess.run([fixture, command, fixture_tag], env=fixture_environment,
                                capture_output=True, timeout=20,
                                creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        if result.returncode != 0 or result.stdout.strip() != b'ok':
            raise AssertionError('Owned queue transition failed: ' + command)

    notifications = queue.Queue(maxsize=100)

    class Receiver(http.server.BaseHTTPRequestHandler):
        def setup(self):
            super().setup()
            self.connection.settimeout(2)

        def do_POST(self):
            try:
                size = int(self.headers.get('Content-Length', '0'))
                if self.path != '/alerts' or not 0 < size <= 65536:
                    raise ValueError('invalid local notification')
                payload = json.loads(self.rfile.read(size))
                if not isinstance(payload, dict) or not isinstance(payload.get('alerts'), list):
                    raise ValueError('invalid local notification')
                notifications.put_nowait(payload)
            except (ValueError, queue.Full):
                self.send_error(400)
                return
            self.send_response(200)
            self.end_headers()

        def log_message(self, *_args):
            pass

    with tempfile.TemporaryDirectory(prefix='board-queue-qualification-') as temporary, ExitStack() as cleanup:
        work = Path(temporary)
        # Register before the first mutation so failures also restore SELECT and
        # remove only this fixture's rows; the outer helper owns cluster removal.
        cleanup.callback(transition, 'cleanup')
        transition('init')
        receiver = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Receiver)
        receiver.daemon_threads = True
        threading.Thread(target=receiver.serve_forever, daemon=True).start()
        cleanup.callback(receiver.server_close)
        cleanup.callback(receiver.shutdown)
        metrics_port, prom_port, alert_port = (port() for _ in range(3))
        if len({metrics_port, prom_port, alert_port, receiver.server_port}) != 4:
            raise AssertionError('Ephemeral port collision; rerun qualification')
        instance = f'127.0.0.1:{metrics_port}'
        endpoint = f'http://{instance}'
        token = secrets.token_hex(32)
        token_path = work / 'monitor.token'
        token_path.write_text(token, encoding='ascii')
        token_path.chmod(0o600)
        rules = accelerated_rules((ROOT / 'deploy/monitoring/alerts.yml').read_text())
        if not all(name in rules for name in ('BoardMediaQueuePressure', 'BoardMediaProcessingFailures')):
            raise AssertionError('Production queue alert rules are missing')
        (work / 'alerts.yml').write_text(rules)
        # A single owned target; production rule definitions and bearer semantics.
        prometheus = {
            'global': {'scrape_interval': '1s', 'evaluation_interval': '1s'},
            'rule_files': ['alerts.yml'],
            'alerting': {'alertmanagers': [{'static_configs': [{'targets': [f'127.0.0.1:{alert_port}']}]}]},
            'scrape_configs': [{'job_name': 'board-monitor',
                                'authorization': {'type': 'Bearer', 'credentials_file': token_path.as_posix()},
                                'static_configs': [{'targets': [instance]}]}],
        }
        (work / 'prometheus.yml').write_text(json.dumps(prometheus))
        alertmanager = (ROOT / 'deploy/monitoring/alertmanager.yml').read_text()
        for before, after in (("127.0.0.1:9095", f'127.0.0.1:{receiver.server_port}'),
                              ('group_wait: 30s', 'group_wait: 1s'), ('group_interval: 5m', 'group_interval: 1s')):
            if alertmanager.count(before) != 1:
                raise AssertionError('Candidate notification configuration changed')
            alertmanager = alertmanager.replace(before, after)
        (work / 'alertmanager.yml').write_text(alertmanager)
        for command in ([tools['promtool'], 'check', 'config', work / 'prometheus.yml'],
                        [tools['amtool'], 'check-config', work / 'alertmanager.yml']):
            result = subprocess.run(command, env=minimal, capture_output=True, timeout=15)
            if result.returncode:
                raise AssertionError('Rendered monitoring configuration was rejected')
        children = []

        def launch(name, command, environment=None):
            output = cleanup.enter_context((work / (name + '.log')).open('wb'))
            process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                                       env=environment if environment is not None else minimal,
                                       creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
            cleanup.callback(stop, process)
            children.append((name, process))
            return process

        monitor_process = launch('monitor', [monitor], dict(minimal, APP_ENV='development',
                                 MONITOR_DATABASE_URL=os.environ['MONITOR_DATABASE_URL'],
                                 METRICS_BIND_ADDR=instance, METRICS_TOKEN=token))
        launch('alertmanager', [tools['alertmanager'], f'--config.file={work / "alertmanager.yml"}',
               f'--storage.path={work / "alert-data"}', f'--web.listen-address=127.0.0.1:{alert_port}',
               '--cluster.listen-address=', '--log.level=warn'])
        launch('prometheus', [tools['prometheus'], f'--config.file={work / "prometheus.yml"}',
               f'--storage.tsdb.path={work / "prom-data"}', '--storage.tsdb.retention.time=1h',
               '--storage.tsdb.retention.size=32MB', f'--web.listen-address=127.0.0.1:{prom_port}', '--log.level=warn'])

        def exposition():
            status, body = request(endpoint + '/metrics', token)
            if status != 200:
                raise AssertionError('Authenticated monitor scrape failed')
            if token.encode() in body or fixture_tag.encode() in body or b'postgres' in body:
                raise AssertionError('Private fixture data appeared in exposition')
            return body

        def sampled(expected):
            values = sample_values(exposition())
            return values if all(values.get(key) == value for key, value in expected.items()) else None

        wait_for('real monitor readiness', lambda: request(endpoint + '/readyz', token)[0] == 200, children)
        if request(endpoint + '/healthz', token)[0] != 200:
            raise AssertionError('Ready monitor did not report process health')
        if request(endpoint + '/metrics')[0] != 401 or request(endpoint + '/readyz')[0] != 401:
            raise AssertionError('Monitor private routes allowed missing authentication')
        wrong = ('0' if token[0] != '0' else '1') + token[1:]
        if request(endpoint + '/metrics', wrong)[0] != 401:
            raise AssertionError('Monitor accepted an incorrect token')
        empty = {'board_media_sample_success': 1, 'board_media_queue_capacity': 4,
                 **{f'board_media_jobs{{state="{state}"}}': 0 for state in ('receiving', 'queued', 'processing')}}
        wait_for('healthy empty queue snapshot', lambda: sampled(empty), children)
        checked = subprocess.run([tools['promtool'], 'check', 'metrics'], input=exposition(),
                                 env=minimal, capture_output=True, timeout=15)
        if checked.returncode:
            raise AssertionError('Monitor exposition was rejected by promtool')
        wait_for('Prometheus API readiness', lambda: request(f'http://127.0.0.1:{prom_port}/-/ready')[0] == 200, children)

        def query(expression):
            status, body = request(f'http://127.0.0.1:{prom_port}/api/v1/query?' + urllib.parse.urlencode({'query': expression}))
            if status != 200:
                raise AssertionError('Prometheus query was unavailable')
            result = json.loads(body)
            if result.get('status') != 'success':
                raise AssertionError('Prometheus query failed')
            return result['data']['result']

        wait_for('authenticated monitor scrape', lambda: query('up{job="board-monitor"} == 1'), children)
        if query('ALERTS{alertname=~"BoardMediaQueuePressure|BoardMediaProcessingFailures"}'):
            raise AssertionError('Healthy queue unexpectedly alerts')
        print('PASS real monitor, aggregate snapshot, private health and authenticated Prometheus scrape', flush=True)
        if lifecycle_state is not None:
            write_lifecycle_state(lifecycle_state, work, [process.pid for _, process in children])
        seen = []

        def notification(name, status):
            while True:
                try:
                    payload = notifications.get_nowait()
                except queue.Empty:
                    break
                seen.extend(payload['alerts'])
                if len(seen) > 200:
                    raise AssertionError('Unexpected notification volume')
            return find_notification(seen, name, status, instance)

        def delivered(name, status):
            alert = wait_for(name + ' ' + status + ' webhook', lambda: notification(name, status), children, 60)
            firing = query('ALERTS{alertname="' + name + '",alertstate="firing",job="board-monitor"}')
            if bool(firing) != (status == 'firing'):
                raise AssertionError('Delivered notification disagrees with Prometheus alert state')
            return alert

        transition('saturate')
        wait_for('four real reservations occupy capacity', lambda: sampled({**empty, 'board_media_jobs{state="receiving"}': 4}), children)
        pressure = delivered('BoardMediaQueuePressure', 'firing')
        transition('drain')
        wait_for('reservation release restores capacity', lambda: sampled(empty), children)
        assert_same_alert(pressure, delivered('BoardMediaQueuePressure', 'resolved'))
        print('PASS real queue admission, saturation firing and resolved delivery after reservation release', flush=True)

        transition('fail')
        failure_key = 'board_media_failures_recent{reason="processing_failed"}'
        wait_for('real processing failure sampled', lambda: sampled({**empty, failure_key: 1}), children)
        failure = delivered('BoardMediaProcessingFailures', 'firing')
        transition('age-failures')
        wait_for('production failure window excludes aged rows', lambda: sampled({**empty, failure_key: 0}), children)
        assert_same_alert(failure, delivered('BoardMediaProcessingFailures', 'resolved'))
        print('PASS real processing-failure transition, firing and recovery through unchanged 15-minute SQL window', flush=True)

        transition('revoke')

        def unavailable():
            status = request(endpoint + '/readyz', token)[0]
            body = exposition()
            if status == 503 and sample_values(body).get('board_media_sample_success') == 0:
                assert_unavailable(status, body)
                if request(endpoint + '/healthz', token)[0] != 200:
                    raise AssertionError('Sampling failure incorrectly removed process health')
                return True
            return False

        wait_for('revoked SELECT removes readiness and queue values', unavailable, children)
        wait_for('Prometheus observes unavailable sample', lambda: query('board_media_sample_success{job="board-monitor"} == 0'), children)
        for family in QUEUE_FAMILIES:
            if query(family + '{job="board-monitor"}'):
                raise AssertionError('Prometheus retained unavailable queue samples')
        transition('restore')
        wait_for('restored SELECT recovers readiness', lambda: request(endpoint + '/readyz', token)[0] == 200, children)
        wait_for('restored SELECT recovers real queue values', lambda: sampled({**empty, failure_key: 0}), children)
        wait_for('Prometheus observes restored sample', lambda: query('board_media_sample_success{job="board-monitor"} == 1'), children)
        print('PASS revoked aggregate SELECT causes sample_success=0, readiness=503 and omitted queue families; regrant recovers', flush=True)
        stop(monitor_process)
        if monitor_process.returncode != 0:
            raise AssertionError('Monitor did not shut down cleanly')
    print('PASS owned monitoring children, queue rows, grants, capacity and temporary credentials cleaned', flush=True)


if __name__ == '__main__':
    install_signal_cleanup()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lifecycle-state', type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    qualify(args.lifecycle_state)
