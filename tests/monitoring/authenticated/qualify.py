"""Qualify authenticated monitoring links using only owned loopback services."""

import argparse
import base64
from contextlib import ExitStack
import http.client
import json
import os
from pathlib import Path
import queue
import re
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "scripts/monitoring"))
from auth_profile import render
from support import Receiver, context, make_pki, request, write_policy

HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def wait_for(description, predicate, children, seconds=40):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        for name, process in children:
            if process.poll() is not None:
                raise AssertionError(f"{name} exited with {process.returncode}")
        try:
            result = predicate()
            if result:
                return result
        except (OSError, urllib.error.URLError, http.client.HTTPException):
            pass
        time.sleep(0.1)
    raise AssertionError(f"Timed out waiting for {description}")


def port():
    with socket.socket() as owned:
        owned.bind(("127.0.0.1", 0))
        return owned.getsockname()[1]


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def install_signal_cleanup():
    def terminate(signum, _frame):
        raise SystemExit(128 + signum)
    signal.signal(signal.SIGTERM, terminate)


def atomic_json(path, value):
    temporary = path.with_suffix(".replacement")
    with temporary.open("x", encoding="utf-8") as output:
        temporary.chmod(0o600)
        json.dump(value, output)
    temporary.replace(path)


def plain_request(url, token=None):
    headers = {"Authorization": f"Bearer {token}"} if token else {}
    try:
        with HTTP.open(urllib.request.Request(url, headers=headers), timeout=2) as response:
            return response.status, response.read(1024 * 1024)
    except urllib.error.HTTPError as error:
        return error.code, error.read(65536)


def qualify(binary_directory, fixture, openssl, lifecycle_state=None):
    suffix = ".exe" if os.name == "nt" else ""
    tools = {name: binary_directory / (name + suffix)
             for name in ("prometheus", "promtool", "alertmanager", "amtool")}
    system_keys = {"systemroot", "windir", "path", "temp", "tmp", "tmpdir"}
    environment = {key: value for key, value in os.environ.items() if key.lower() in system_keys}
    if not all(binary.is_file() for binary in [fixture, *tools.values()]):
        raise AssertionError("Required owned qualification binary missing")

    with tempfile.TemporaryDirectory(prefix="board-auth-monitor-") as directory, ExitStack() as cleanup:
        work = Path(directory).resolve()
        work.chmod(0o700)
        pki = make_pki(work / "pki", openssl)
        credentials = {name: secrets.token_hex(32) for name in
                       ("prom-operator", "alert-ingest", "alert-operator", "receiver", "scrape")}
        paths = {}
        for name, value in credentials.items():
            path = work / (name + ".token")
            with path.open("x", encoding="ascii") as output:
                path.chmod(0o600)
                output.write(value)
            paths[name] = path
        receiver_policy = work / "receiver-policy.json"
        write_policy(receiver_policy, credentials["receiver"])
        receiver = cleanup.enter_context(Receiver(pki["cert"], pki["key"], receiver_policy))
        metrics_port, app_port, prom_port, alert_port = (port() for _ in range(4))
        if len({metrics_port, app_port, prom_port, alert_port, receiver.port}) != 5:
            raise AssertionError("Owned ephemeral port collision")
        common = {"server_name": "localhost", "ca_file": str(pki["ca"]),
                  "cert_file": str(pki["cert"]), "key_file": str(pki["key"])}
        manifest = {
            "prometheus": dict(common, listen=f"127.0.0.1:{prom_port}",
                               operator_password_file=str(paths["prom-operator"])),
            "alertmanager": dict(common, listen=f"127.0.0.1:{alert_port}",
                                 ingest_password_file=str(paths["alert-ingest"]),
                                 operator_password_file=str(paths["alert-operator"])),
            "receiver": {"url": receiver.url, "ca_file": str(pki["ca"]), "token_file": str(paths["receiver"])},
            "scrapes": [{"job": "board-public", "target": f"127.0.0.1:{metrics_port}", "token_file": str(paths["scrape"])}],
            "rules_file": str((ROOT / "deploy/monitoring/alerts.yml").resolve()),
        }
        configs = render(manifest, work / "profile")

        def native_check(command):
            result = subprocess.run(command, capture_output=True, timeout=20, env=environment,
                                    creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
            if result.returncode:
                raise AssertionError("Native monitoring configuration validation failed")

        native_check([tools["promtool"], "check", "config", configs["prometheus"]])
        native_check([tools["promtool"], "check", "web-config", configs["prometheus_web"], configs["alertmanager_web"]])
        native_check([tools["amtool"], "check-config", configs["alertmanager"]])
        print("PASS native validation of unmodified generated profile", flush=True)

        # Preserve all production expressions/thresholds/group labels. Accelerate
        # only transport qualification timing, as in the existing HTTP harness.
        rules = (ROOT / "deploy/monitoring/alerts.yml").read_text().replace("[5m]", "[30s]").replace("for: 2m", "for: 4s")
        (work / "alerts.yml").write_text(rules, encoding="utf-8")
        prometheus = json.loads(configs["prometheus"].read_text())
        prometheus["global"].update(scrape_interval="1s", evaluation_interval="1s")
        prometheus["rule_files"] = [str(work / "alerts.yml")]
        atomic_json(configs["prometheus"], prometheus)
        alertmanager = json.loads(configs["alertmanager"].read_text())
        alertmanager["route"].update(group_wait="1s", group_interval="1s")
        atomic_json(configs["alertmanager"], alertmanager)
        children = []

        def launch(name, command, child_environment=None):
            output = cleanup.enter_context((work / (name + ".log")).open("wb"))
            process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT,
                                       env=child_environment if child_environment is not None else environment,
                                       creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
            cleanup.callback(stop, process)
            children.append((name, process))

        launch("exporter", [fixture], dict(environment, METRICS_BIND_ADDR=f"127.0.0.1:{metrics_port}",
                                          METRICS_TOKEN=credentials["scrape"], FIXTURE_BIND_ADDR=f"127.0.0.1:{app_port}"))
        launch("alertmanager", [tools["alertmanager"], f"--config.file={configs['alertmanager']}",
                                f"--web.config.file={configs['alertmanager_web']}", f"--storage.path={work / 'alert-data'}",
                                f"--web.listen-address=127.0.0.1:{alert_port}", "--cluster.listen-address=", "--log.level=warn"])
        launch("prometheus", [tools["prometheus"], f"--config.file={configs['prometheus']}",
                              f"--web.config.file={configs['prometheus_web']}", f"--storage.tsdb.path={work / 'prom-data'}",
                              "--storage.tsdb.retention.time=1h", "--storage.tsdb.retention.size=32MB",
                              f"--web.listen-address=127.0.0.1:{prom_port}", "--log.level=warn"])
        app = f"http://127.0.0.1:{app_port}"
        prom = f"https://localhost:{prom_port}"
        am = f"https://localhost:{alert_port}"
        prom_basic = ("operator", credentials["prom-operator"])
        am_basic = ("operator", credentials["alert-operator"])
        ingest_basic = ("prometheus", credentials["alert-ingest"])
        wait_for("fixture readiness", lambda: plain_request(app + "/health")[0] == 200, children)
        for name, url, basic in (("Prometheus", prom + "/api/v1/status/runtimeinfo", prom_basic),
                                 ("Alertmanager", am + "/api/v2/status", am_basic)):
            wait_for(name + " HTTPS readiness", lambda: request(url, pki["ca"], basic=basic)[0] == 200, children)
            for bad in (None, (basic[0], "0" * 64)):
                if request(url, pki["ca"], basic=bad)[0] != 401:
                    raise AssertionError(name + " did not reject missing/wrong Basic credentials")
        print("PASS both native HTTPS APIs require independent Basic credentials", flush=True)

        original_web = json.loads(configs["alertmanager_web"].read_text())
        revoked_web = json.loads(json.dumps(original_web))
        revoked_web["basic_auth_users"]["prometheus"] = original_web["basic_auth_users"]["operator"]
        connection = http.client.HTTPSConnection("localhost", alert_port, context=context(pki["ca"]), timeout=3)
        cleanup.callback(connection.close)
        basic_header = "Basic " + base64.b64encode(":".join(ingest_basic).encode("ascii")).decode("ascii")

        def persistent(expected, previous_socket=None):
            connection.request("GET", "/api/v2/alerts", headers={"Authorization": basic_header})
            response = connection.getresponse()
            response.read(1024 * 1024)
            if response.status != expected or connection.sock is None:
                raise AssertionError("Native request policy returned an unexpected result")
            if previous_socket is not None and connection.sock is not previous_socket:
                raise AssertionError("Native revocation test did not reuse the established TLS connection")
            return connection.sock

        established = persistent(200)
        atomic_json(configs["alertmanager_web"], revoked_web)
        persistent(401, established)
        saved_web = configs["alertmanager_web"].with_suffix(".unavailable")
        configs["alertmanager_web"].replace(saved_web)
        try:
            persistent(500, established)
        finally:
            saved_web.replace(configs["alertmanager_web"])
        atomic_json(configs["alertmanager_web"], original_web)
        persistent(200, established)
        connection.close()
        print("PASS established TLS connection rejects revoked/unavailable policy and recovers", flush=True)

        def query(expression):
            status, body = request(prom + "/api/v1/query?" + urllib.parse.urlencode({"query": expression}), pki["ca"], basic=prom_basic)
            if status != 200:
                raise AssertionError("Authenticated Prometheus query was unavailable")
            payload = json.loads(body)
            if payload["status"] != "success":
                raise AssertionError("Authenticated Prometheus query failed")
            return payload["data"]["result"]

        def failed_notifications(url, basic, metric):
            status, body = request(url + "/metrics", pki["ca"], basic=basic)
            if status != 200:
                raise AssertionError("Authenticated notification counters were unavailable")
            pattern = rb"^" + metric.encode("ascii") + rb"(?:\{[^\n]*\})? ([0-9.eE+-]+)$"
            return sum(float(match) for match in re.findall(pattern, body, re.MULTILINE))

        def am_alerts():
            status, body = request(am + "/api/v2/alerts", pki["ca"], basic=am_basic)
            if status != 200:
                raise AssertionError("Authenticated Alertmanager observation was unavailable")
            return [alert for alert in json.loads(body) if alert.get("labels", {}).get("alertname") == "BoardHttp5xx"]

        wait_for("authenticated Prometheus scrape", lambda: query('up{job="board-public"} == 1'), children)
        if query('ALERTS{alertname="BoardHttp5xx"}'):
            raise AssertionError("Healthy fixture unexpectedly alerts")
        # TLS/API observations can be slow independently of application traffic.
        # Keep a bounded real request stream above the unchanged >=1/s rule gate.
        traffic_stop = threading.Event()
        failing_traffic = threading.Event()
        traffic_failed = threading.Event()

        def traffic():
            try:
                while not traffic_stop.is_set():
                    failing = failing_traffic.is_set()
                    if plain_request(app + ("/failure" if failing else "/health"))[0] != (500 if failing else 200):
                        traffic_failed.set()
                        return
                    traffic_stop.wait(0.1)
            except (OSError, urllib.error.URLError, http.client.HTTPException):
                traffic_failed.set()
            finally:
                if not traffic_stop.is_set():
                    traffic_failed.set()

        traffic_thread = threading.Thread(target=traffic, name="monitor-fixture-traffic", daemon=False)
        traffic_thread.start()

        def stop_traffic():
            traffic_stop.set()
            traffic_thread.join(timeout=4)
            if traffic_thread.is_alive():
                raise AssertionError("Owned fixture traffic thread failed to stop")

        cleanup.callback(stop_traffic)

        def drive_failure():
            failing_traffic.set()
            if traffic_failed.is_set():
                raise AssertionError("Real Rust fixture traffic failed")

        print("PASS authenticated real Rust scrape over the generated profile", flush=True)
        if lifecycle_state is not None:
            atomic_json(lifecycle_state, {"directory": str(work), "children": [process.pid for _, process in children],
                                          "receiver_port": receiver.port})

        # Prove an actual denied send on each link, with a live firing control.
        ingest_failures_before = failed_notifications(prom, prom_basic, "prometheus_notifications_errors_total")
        atomic_json(configs["alertmanager_web"], revoked_web)

        def blocked_ingest():
            drive_failure()
            return query('ALERTS{alertname="BoardHttp5xx",alertstate="firing"}') and failed_notifications(
                prom, prom_basic, "prometheus_notifications_errors_total") > ingest_failures_before

        try:
            wait_for("real rejected Prometheus alert delivery", blocked_ingest, children, 70)
        except AssertionError:
            print(json.dumps({"stage": "ingest rejection", "firing": len(query('ALERTS{alertname="BoardHttp5xx",alertstate="firing"}')),
                              "request_rate": [sample["value"][1] for sample in query('sum(rate(board_http_responses_total[30s]))')],
                              "notification_failures": failed_notifications(prom, prom_basic, "prometheus_notifications_errors_total")}), flush=True)
            raise
        if am_alerts() or not receiver.notifications.empty():
            raise AssertionError("Revoked ingest credential allowed alert delivery")
        print("PASS revoked ingest prevents actual firing alert from entering Alertmanager", flush=True)
        receiver_failures_before = failed_notifications(am, am_basic, "alertmanager_notifications_failed_total")
        write_policy(receiver_policy, credentials["receiver"], active=False)
        atomic_json(configs["alertmanager_web"], original_web)

        def blocked_receiver():
            drive_failure()
            return am_alerts() and failed_notifications(am, am_basic, "alertmanager_notifications_failed_total") > receiver_failures_before

        wait_for("real rejected Alertmanager notification", blocked_receiver, children, 70)
        if not receiver.notifications.empty():
            raise AssertionError("Revoked receiver credential allowed a notification")
        print("PASS restored ingest reaches Alertmanager; revoked receiver rejects actual delivery", flush=True)
        write_policy(receiver_policy, credentials["receiver"])
        seen = []

        def notification(status, route, expected):
            if route == "/failure":
                failing_traffic.set()
            else:
                failing_traffic.clear()
            if traffic_failed.is_set():
                raise AssertionError("Real Rust fixture traffic failed")
            while True:
                try:
                    payload = receiver.notifications.get_nowait()
                except queue.Empty:
                    break
                for alert in payload.get("alerts", []):
                    if alert.get("labels", {}).get("alertname") == "BoardHttp5xx":
                        seen.append(alert)
            for alert in seen:
                if alert["status"] == status:
                    if alert["labels"].get("listener") != "public" or alert["labels"].get("job") != "board-public":
                        raise AssertionError("Notification contained unexpected fixture labels")
                    return alert
            return None

        firing = wait_for("authenticated firing notification", lambda: notification("firing", "/failure", 500), children, 70)
        if not query('ALERTS{alertname="BoardHttp5xx",alertstate="firing"}'):
            raise AssertionError("Notification has no matching firing Prometheus alert")
        print("PASS real firing notification through both restored authenticated HTTPS links", flush=True)
        resolved = wait_for("authenticated recovery notification", lambda: notification("resolved", "/health", 200), children, 90)
        if firing["fingerprint"] != resolved["fingerprint"] or query('ALERTS{alertname="BoardHttp5xx",alertstate="firing"}'):
            raise AssertionError("Recovery does not match the original firing alert")
        if traffic_failed.is_set() or not query('sum(rate(board_http_responses_total{status_class="2xx"}[30s])) >= 1'):
            raise AssertionError("Recovery did not retain successful real fixture traffic")
        print("PASS matching resolved notification and nonfiring Prometheus state", flush=True)
        print(json.dumps({"alert": "BoardHttp5xx", "fingerprint": firing["fingerprint"],
                          "firing_started_at": firing["startsAt"], "resolved_at": resolved["endsAt"],
                          "receiver": "owned HTTPS loopback only"}), flush=True)


if __name__ == "__main__":
    install_signal_cleanup()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / ".local/monitoring/bin")
    parser.add_argument("--fixture", type=Path, default=ROOT / "target/debug/examples" / ("http_fixture.exe" if os.name == "nt" else "http_fixture"))
    parser.add_argument("--openssl", default=os.environ.get("MONITOR_TEST_OPENSSL") or shutil.which("openssl"))
    parser.add_argument("--lifecycle-state", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not args.openssl:
        parser.error("OpenSSL is required for owned test PKI")
    qualify(args.bin_dir.resolve(), args.fixture.resolve(), args.openssl, args.lifecycle_state)
