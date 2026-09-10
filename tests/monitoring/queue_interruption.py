"""Linux SIGTERM qualification of real queue-monitoring children and cleanup.

The surrounding disposable PostgreSQL helper checks restored rows, capacity and
SELECT grants after this watcher returns, then removes its own cluster.
The qualifier and its children stay in that helper's process group, including
when this watcher is forcibly terminated before it can run its own cleanup.
"""

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile

from qualify import install_signal_cleanup, wait_for


def main():
    if not sys.platform.startswith('linux'):
        raise SystemExit('OS-delivered queue SIGTERM qualification requires Linux')
    if os.environ.get('QUEUE_QUALIFICATION') != 'owned-disposable':
        raise AssertionError('Use the owned disposable PostgreSQL helper')
    required = ('BOARD_MONITOR_BIN', 'QUEUE_FIXTURE_BIN', 'MONITORING_BIN_DIR',
                'MONITOR_DATABASE_URL', 'MEDIA_DATABASE_URL', 'MIGRATION_DATABASE_URL')
    environment = {key: os.environ[key] for key in required}
    environment.update({key: value for key, value in os.environ.items() if key == 'PATH'})
    environment['QUEUE_QUALIFICATION'] = 'owned-disposable'
    owned_group = os.getpgrp()
    owned_session = os.getsid(0)
    with tempfile.TemporaryDirectory(prefix='board-queue-interruption-') as temporary:
        work = Path(temporary).resolve()
        state_path = work / 'state.json'
        environment['TMPDIR'] = str(work)
        process = None
        cleanup_verified = False
        with (work / 'qualification.log').open('wb') as output:
            try:
                process = subprocess.Popen(
                    [sys.executable, str(Path(__file__).with_name('queue_qualify.py')),
                     '--lifecycle-state', str(state_path)],
                    stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                    env=environment,
                )
                wait_for('real queue sampling and scrape before interruption', state_path.exists,
                         [('queue qualification', process)], 45)
                state = json.loads(state_path.read_text(encoding='utf-8'))
                children = state['children']
                if (len(children) != 3 or len(set(children)) != 3
                        or not all(isinstance(pid, int) and pid > 0 for pid in children)):
                    raise AssertionError('Qualification did not publish three child identities')
                for pid in (process.pid, *children):
                    if os.getsid(pid) != owned_session or os.getpgid(pid) != owned_group:
                        raise AssertionError('A qualifier process is outside the helper cleanup group')
                fixture_work = Path(state['directory'])
                if (fixture_work.parent != work or fixture_work.resolve() != fixture_work
                        or not fixture_work.name.startswith('board-queue-qualification-')):
                    raise AssertionError('Qualification storage is outside the owned temporary directory')
                if not (fixture_work / 'monitor.token').is_file():
                    raise AssertionError('Credential cleanup was not exercised')
                process.send_signal(signal.SIGTERM)
                if process.wait(timeout=35) != 143:
                    raise AssertionError('Queue qualifier did not unwind through its SIGTERM handler')
                if any(Path(f'/proc/{pid}').exists() for pid in children):
                    raise AssertionError('A monitoring child survived queue SIGTERM cleanup')
                if fixture_work.exists():
                    raise AssertionError('Queue qualification credentials or storage survived SIGTERM')
                cleanup_verified = True
                print('PASS OS SIGTERM removed three real monitoring children and temporary queue credentials', flush=True)
            except BaseException:
                # The verifier reports static failures. Child diagnostics remain
                # private and are removed with the owned temporary directory.
                print('Queue interruption qualification failed; the outer helper owns group cleanup', flush=True)
                raise
            finally:
                if process is not None and not cleanup_verified and process.poll() is None:
                    # Stop only our still-running direct child. A failure leaves
                    # the helper's wait nonzero; it then cleans the entire owned
                    # group, including descendants if this watcher is killed.
                    process.kill()
                    process.wait(timeout=5)


if __name__ == '__main__':
    install_signal_cleanup()
    main()
