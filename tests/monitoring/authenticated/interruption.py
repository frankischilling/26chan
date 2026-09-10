"""Linux SIGTERM evidence for the actual authenticated monitoring qualifier."""

import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile

import qualify


def anchor(directory):
    # Retain the session leader after qualifier failure, so group cleanup never
    # uses a retired leader PID. This supervisor has no application credentials.
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    child = subprocess.Popen([sys.executable, str(Path(__file__).with_name("qualify.py")),
                              "--lifecycle-state", str(directory / "state.json")])
    qualify.atomic_json(directory / "pid.json", child.pid)
    result = child.wait()
    qualify.atomic_json(directory / "result.json", result)
    if sys.stdin.buffer.read(1) != b"q":
        os.killpg(os.getpid(), signal.SIGKILL)


def main():
    if not sys.platform.startswith("linux"):
        raise SystemExit("OS-delivered SIGTERM qualification requires Linux")
    qualify.install_signal_cleanup()
    with tempfile.TemporaryDirectory(prefix="board-auth-interruption-") as directory:
        work_root = Path(directory).resolve()
        state_path = work_root / "state.json"
        result_path = work_root / "result.json"
        log_path = work_root / "qualification.log"
        supervisor = None
        with log_path.open("wb") as log:
            try:
                supervisor = subprocess.Popen(
                    [sys.executable, str(Path(__file__)), "--anchor", str(work_root)],
                    stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT, start_new_session=True,
                    env=dict({key: value for key, value in os.environ.items() if key == "PATH"}, TMPDIR=directory),
                )

                def healthy_state():
                    if result_path.exists():
                        raise AssertionError("Qualifier exited before the actual healthy scrape")
                    return state_path.exists()

                qualify.wait_for("authenticated real scrape before interruption", healthy_state,
                                 [("owned supervisor", supervisor)], 60)
                state = json.loads(state_path.read_text())
                qualifier_pid = json.loads((work_root / "pid.json").read_text())
                children = state["children"]
                if len(children) != 3 or len(set(children)) != 3:
                    raise AssertionError("Qualification did not start three distinct children")
                for pid in [supervisor.pid, qualifier_pid, *children]:
                    if os.getpgid(pid) != supervisor.pid or os.getsid(pid) != supervisor.pid:
                        raise AssertionError("A qualifier process escaped the owned process group")
                work = Path(state["directory"]).resolve()
                if work.parent != work_root or not (work / "scrape.token").is_file() or not (work / "pki").is_dir():
                    raise AssertionError("Owned temporary credentials/PKI were not exercised")
                os.kill(qualifier_pid, signal.SIGTERM)
                qualify.wait_for("qualifier SIGTERM exit", result_path.exists, [("owned supervisor", supervisor)], 35)
                if json.loads(result_path.read_text()) != 143:
                    raise AssertionError("Qualification did not unwind through the SIGTERM handler")
                if any(Path(f"/proc/{pid}").exists() for pid in [qualifier_pid, *children]):
                    raise AssertionError("An owned qualification process survived SIGTERM cleanup")
                if work.exists():
                    raise AssertionError("Temporary credentials/PKI/storage survived SIGTERM cleanup")
                with socket.socket() as probe:
                    probe.settimeout(1)
                    if probe.connect_ex(("127.0.0.1", state["receiver_port"])) == 0:
                        raise AssertionError("Owned HTTPS receiver listener survived cleanup")
                supervisor.stdin.write(b"q")
                supervisor.stdin.flush()
                if supervisor.wait(timeout=5) != 0:
                    raise AssertionError("Owned supervisor did not exit after cleanup acknowledgement")
                print("PASS OS SIGTERM cleaned three real children, HTTPS receiver, private PKI and storage")
            except BaseException:
                with log_path.open("rb") as saved:
                    saved.seek(max(0, log_path.stat().st_size - 8192))
                    print(saved.read().decode(errors="replace"))
                raise
            finally:
                if supervisor is not None:
                    if supervisor.poll() is None:
                        os.killpg(supervisor.pid, signal.SIGKILL)
                        supervisor.wait(timeout=5)
                    if supervisor.stdin is not None:
                        supervisor.stdin.close()


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--anchor":
        anchor(Path(sys.argv[2]))
    else:
        main()
