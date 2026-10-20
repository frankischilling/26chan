#!/usr/bin/env python3
"""Qualify startup failure evidence without a database or network listener."""
import os
import re
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


FIXTURES = {
    "test-global-announcement-migration.sh": 15454,
    "test-homepage-migration.sh": 15446,
    "test-janitor-request-store.sh": 15449,
    "test-janitor-request-review.sh": 15450,
    "test-staff-messages-migration.sh": 15447,
    "test-janitor-request-authority.sh": 15448,
}


class OwnedDatabaseRunnerTests(unittest.TestCase):
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
            for name in FIXTURES:
                with self.subTest(fixture=name):
                    result = subprocess.run(
                        ["bash", f"scripts/{name}"],
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
        for name, port in FIXTURES.items():
            with self.subTest(fixture=name):
                source = (ROOT / "scripts" / name).read_text()
                self.assertLess(port, 32768)
                self.assertEqual(set(re.findall(r"(?<!\d)(?:15|55)\d{3}(?!\d)", source)), {str(port)})
                self.assertIn(f"port={port}", source)
                self.assertIn(f"-p {port}", source)



if __name__ == "__main__":
    unittest.main()
