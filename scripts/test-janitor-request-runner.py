#!/usr/bin/env python3
"""Qualify startup failure evidence without a database or network listener."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class RequestAuthorityRunnerTests(unittest.TestCase):
    def test_startup_failure_reports_log_and_cleans_owned_cluster(self):
        with tempfile.TemporaryDirectory(prefix="request-runner-tools-") as directory:
            tools = Path(directory)
            tools.chmod(0o755)
            programs = {
                "initdb": "mkdir -p \"$2\"\n",
                "pg_ctl": '''case "${@: -1}" in
start)
    log=''
    while (($#)); do
        if [[ $1 = -l ]]; then log=$2; shift; fi
        shift
    done
    printf 'owned startup diagnostic\\n' > "$log"
    echo "owned-cluster=${log%/server.log}" >&2
    exit 1;;
status) exit 3;;
stop) exit 1;;
*) exit 99;;
esac
''',
            }
            for name, body in programs.items():
                executable = tools / name
                executable.write_text("#!/usr/bin/env bash\nset -eu\n" + body)
                executable.chmod(0o755)
            result = subprocess.run(
                ["bash", "scripts/test-janitor-request-authority.sh"],
                cwd=ROOT, env={**os.environ, "PG_BIN": str(tools)},
                text=True, capture_output=True, timeout=15,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("owned startup diagnostic", result.stderr)
            clusters = [line.split("=", 1)[1] for line in result.stderr.splitlines()
                        if line.startswith("owned-cluster=")]
            self.assertEqual(len(clusters), 1, result.stderr)
            self.assertFalse(Path(clusters[0]).exists())

    def test_listener_and_clients_use_one_non_ephemeral_port(self):
        source = (ROOT / "scripts/test-janitor-request-authority.sh").read_text()
        self.assertNotIn("55448", source)
        self.assertEqual(source.count("15448"), 4)


if __name__ == "__main__":
    unittest.main()
