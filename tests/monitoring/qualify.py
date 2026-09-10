"""Qualify real exporter -> Prometheus -> Alertmanager -> owned local webhook."""

import argparse
from contextlib import ExitStack
import http.server
import json
import os
from pathlib import Path
import queue
import secrets
import signal
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
# Bypass environment proxy settings: every HTTP request in this test is loopback.
HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def install_signal_cleanup():
    def terminate(signum, _frame):
        raise SystemExit(128 + signum)

    signal.signal(signal.SIGTERM, terminate)


def request(url, token=None):
    headers = {"Authorization": f"Bearer {token}"} if token else {}
    try:
        with HTTP.open(urllib.request.Request(url, headers=headers), timeout=2) as response:
            return response.status, response.read(1024 * 1024)
    except urllib.error.HTTPError as error:
        return error.code, error.read(65536)


def port():
    with socket.socket() as owned:
        owned.bind(("127.0.0.1", 0))
        return owned.getsockname()[1]


def wait_for(description, predicate, children, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        for name, process in children:
            if process.poll() is not None:
                raise AssertionError(f"{name} exited with {process.returncode}")
        try:
            result = predicate()
            if result:
                return result
        except (OSError, urllib.error.URLError):
            pass
        time.sleep(0.1)
    raise AssertionError(f"Timed out waiting for {description}")


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def qualify(binary_directory, fixture, lifecycle_state=None):
    suffix = ".exe" if os.name == "nt" else ""
    tools = {name: binary_directory / (name + suffix) for name in ("prometheus", "promtool", "alertmanager", "amtool")}
    # Child tools need OS paths, not the caller's application credentials.
    system_keys = {"systemroot", "windir", "path", "temp", "tmp", "tmpdir"}
    system_environment = {key: value for key, value in os.environ.items() if key.lower() in system_keys}
    for binary in [fixture, *tools.values()]:
        if not binary.is_file():
            raise AssertionError(f"Required binary missing: {binary}")
    notifications = queue.Queue(maxsize=100)

    class Receiver(http.server.BaseHTTPRequestHandler):
        def setup(self):
            super().setup()
            self.connection.settimeout(2)

        def do_POST(self):
            size = int(self.headers.get("Content-Length", "0"))
            if self.path != "/alerts" or not 0 < size <= 65536:
                self.send_error(400)
                return
            try:
                payload = json.loads(self.rfile.read(size))
                notifications.put_nowait(payload)
            except (ValueError, queue.Full):
                self.send_error(400)
                return
            self.send_response(200)
            self.end_headers()

        def log_message(self, *_args):
            pass

    with tempfile.TemporaryDirectory(prefix="board-monitor-qualification-") as directory, ExitStack() as cleanup:
        work = Path(directory)
        receiver = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Receiver)
        receiver.daemon_threads = True
        receiver_thread = threading.Thread(target=receiver.serve_forever, daemon=True)
        receiver_thread.start()
        cleanup.callback(receiver.server_close)
        cleanup.callback(receiver.shutdown)
        metrics_port, app_port, prom_port, alert_port = (port() for _ in range(4))
        if len({metrics_port, app_port, prom_port, alert_port, receiver.server_port}) != 5:
            raise AssertionError("Ephemeral port collision; rerun qualification")
        token = secrets.token_hex(32)
        token_path = work / "metrics.token"
        token_path.write_text(token, encoding="ascii")
        token_path.chmod(0o600)
        # Same production expressions and grouping; only time constants accelerate.
        rules = (ROOT / "deploy/monitoring/alerts.yml").read_text().replace("[5m]", "[30s]").replace("for: 2m", "for: 4s")
        (work / "alerts.yml").write_text(rules)
        prometheus = (ROOT / "deploy/monitoring/prometheus.yml").read_text().split("  - job_name: board-staff")[0]
        prometheus = prometheus.replace("15s", "1s").replace("127.0.0.1:9093", f"127.0.0.1:{alert_port}").replace("127.0.0.1:9191", f"127.0.0.1:{metrics_port}").replace("/etc/26chan/metrics/public.token", token_path.as_posix())
        (work / "prometheus.yml").write_text(prometheus)
        alertmanager = (ROOT / "deploy/monitoring/alertmanager.yml").read_text().replace("127.0.0.1:9095", f"127.0.0.1:{receiver.server_port}").replace("group_wait: 30s", "group_wait: 1s").replace("group_interval: 5m", "group_interval: 1s")
        (work / "alertmanager.yml").write_text(alertmanager)
        for command in (
            [tools["promtool"], "check", "config", work / "prometheus.yml"],
            [tools["amtool"], "check-config", work / "alertmanager.yml"],
        ):
            subprocess.run(command, check=True, timeout=15, env=system_environment)

        children = []
        logs = []

        def launch(name, command, environment=None):
            log_path = work / f"{name}.log"
            output = cleanup.enter_context(log_path.open("wb"))
            process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT,
                                       env=environment if environment is not None else system_environment,
                                       creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
            cleanup.callback(stop, process)
            children.append((name, process))
            logs.append(log_path)

        try:
            environment = dict(system_environment, METRICS_BIND_ADDR=f"127.0.0.1:{metrics_port}", METRICS_TOKEN=token, FIXTURE_BIND_ADDR=f"127.0.0.1:{app_port}")
            launch("exporter", [fixture], environment)
            launch("alertmanager", [tools["alertmanager"], f"--config.file={work / 'alertmanager.yml'}", f"--storage.path={work / 'alert-data'}", f"--web.listen-address=127.0.0.1:{alert_port}", "--cluster.listen-address=", "--log.level=warn"])
            launch("prometheus", [tools["prometheus"], f"--config.file={work / 'prometheus.yml'}", f"--storage.tsdb.path={work / 'prom-data'}", "--storage.tsdb.retention.time=1h", "--storage.tsdb.retention.size=32MB", f"--web.listen-address=127.0.0.1:{prom_port}", "--log.level=warn"])
            app = f"http://127.0.0.1:{app_port}"
            metrics_url = f"http://127.0.0.1:{metrics_port}/metrics"
            wait_for("fixture readiness", lambda: request(app + "/health")[0] == 200, children)
            if request(metrics_url)[0] != 401:
                raise AssertionError("Exporter did not reject an unauthenticated scrape with 401")
            wrong_token = ("0" if token[0] != "0" else "1") + token[1:]
            if request(metrics_url, wrong_token)[0] != 401:
                raise AssertionError("Exporter did not reject an incorrect token with 401")
            status, exposition = request(metrics_url, token)
            if status != 200:
                raise AssertionError(f"Authenticated scrape returned {status}")
            subprocess.run([tools["promtool"], "check", "metrics"], input=exposition, check=True, timeout=15, env=system_environment)
            print("PASS authenticated real Rust exporter and exposition syntax", flush=True)

            def query(expression):
                status, body = request(f"http://127.0.0.1:{prom_port}/api/v1/query?" + urllib.parse.urlencode({"query": expression}))
                if status != 200:
                    return []
                result = json.loads(body)
                if result["status"] != "success":
                    raise AssertionError("Prometheus query failed")
                return result["data"]["result"]

            wait_for("authenticated Prometheus scrape", lambda: query('up{job="board-public"} == 1'), children)
            if query('ALERTS{alertname="BoardHttp5xx"}'):
                raise AssertionError("Healthy fixture unexpectedly alerts")
            print("PASS healthy Prometheus scrape", flush=True)
            if lifecycle_state is not None:
                state = {"directory": str(work), "children": [process.pid for _, process in children]}
                temporary_state = lifecycle_state.with_suffix(".tmp")
                temporary_state.write_text(json.dumps(state))
                temporary_state.replace(lifecycle_state)
            seen = []

            def notification(status, route, expected_status):
                response_status, _ = request(app + route)
                if response_status != expected_status:
                    raise AssertionError(f"Fixture {route} returned {response_status}")
                while True:
                    try:
                        payload = notifications.get_nowait()
                    except queue.Empty:
                        break
                    for alert in payload.get("alerts", []):
                        if alert.get("labels", {}).get("alertname") == "BoardHttp5xx":
                            seen.append(alert)
                for alert in seen:
                    if alert["status"] == status:
                        if alert["labels"].get("listener") != "public" or alert["labels"].get("job") != "board-public":
                            raise AssertionError("Unexpected exporter alert labels")
                        return alert
                return None

            firing = wait_for("5xx firing webhook", lambda: notification("firing", "/failure", 500), children, 60)
            if not query('ALERTS{alertname="BoardHttp5xx",alertstate="firing"}'):
                raise AssertionError("Webhook has no matching firing Prometheus alert")
            print("PASS firing webhook from actual 5xx requests", flush=True)
            resolved = wait_for("5xx resolved webhook", lambda: notification("resolved", "/health", 200), children, 75)
            if firing["fingerprint"] != resolved["fingerprint"]:
                raise AssertionError("Resolved webhook does not identify the firing alert")
            if query('ALERTS{alertname="BoardHttp5xx",alertstate="firing"}'):
                raise AssertionError("Prometheus alert remained firing after recovery")
            print("PASS resolved webhook after healthy requests", flush=True)
            print(json.dumps({"alert": "BoardHttp5xx", "fingerprint": firing["fingerprint"], "firing_started_at": firing["startsAt"], "resolved_at": resolved["endsAt"], "receiver": "owned loopback only"}), flush=True)
        except BaseException:
            for log_path in logs:
                # Bounded diagnostics, with the ephemeral test secret redacted.
                with log_path.open("rb") as log:
                    log.seek(max(0, log_path.stat().st_size - 8192))
                    print(f"{log_path.name}:\n" + log.read().decode(errors="replace").replace(token, "[REDACTED]"))
            raise


if __name__ == "__main__":
    install_signal_cleanup()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / ".local/monitoring/bin")
    parser.add_argument("--fixture", type=Path, default=ROOT / "target/debug/examples" / ("http_fixture.exe" if os.name == "nt" else "http_fixture"))
    parser.add_argument("--lifecycle-state", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    qualify(args.bin_dir.resolve(), args.fixture.resolve(), args.lifecycle_state)
