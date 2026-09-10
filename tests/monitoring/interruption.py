"""Linux: interrupt the actual qualification and verify owned resources disappear."""

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

import qualify


def main():
    if not sys.platform.startswith("linux"):
        raise SystemExit("OS-delivered SIGTERM qualification requires Linux")
    with tempfile.TemporaryDirectory(prefix="board-interruption-") as directory:
        state_path = Path(directory) / "state.json"
        log_path = Path(directory) / "qualification.log"
        process = None
        cleanup_verified = False
        with log_path.open("wb") as log:
            try:
                process = subprocess.Popen(
                    [sys.executable, str(Path(__file__).with_name("qualify.py")), "--lifecycle-state", str(state_path)],
                    stdout=log, stderr=subprocess.STDOUT, start_new_session=True,
                    env=dict({key: value for key, value in os.environ.items() if key == "PATH"}, TMPDIR=directory),
                )
                qualify.wait_for("real scrape before interruption", state_path.exists, [("qualification", process)], 45)
                state = json.loads(state_path.read_text())
                children = state["children"]
                if len(children) != 3 or not all(Path(f"/proc/{pid}").exists() for pid in children):
                    raise AssertionError("Qualification did not start its three real child processes")
                work = Path(state["directory"])
                if not (work / "metrics.token").is_file():
                    raise AssertionError("Credential cleanup was not exercised")
                process.send_signal(signal.SIGTERM)
                if process.wait(timeout=35) != 143:
                    raise AssertionError("Qualification did not unwind through the SIGTERM handler")
                if any(Path(f"/proc/{pid}").exists() for pid in children):
                    raise AssertionError("An owned monitoring process survived SIGTERM cleanup")
                if work.exists():
                    raise AssertionError("Temporary credentials/storage survived SIGTERM cleanup")
                cleanup_verified = True
                print("PASS OS SIGTERM cleaned all three real child processes and temporary credentials")
            except BaseException:
                # Log contains only redacted qualification diagnostics, bounded here.
                with log_path.open("rb") as saved:
                    saved.seek(max(0, log_path.stat().st_size - 8192))
                    print(saved.read().decode(errors="replace"))
                raise
            finally:
                if process is not None and not cleanup_verified:
                    # This test created a new session; cleanup can touch only that
                    # owned process group, never broad process-name matches.
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    process.wait(timeout=5)


if __name__ == "__main__":
    main()
