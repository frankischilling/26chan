"""Retain a live owned supervisor through normal, failed and SIGTERM qualification."""

import argparse
import json
import os
from pathlib import Path
import secrets
import select
import shutil
import signal
import socket
import subprocess
import sys
import time

from fixture import ENVIRONMENT, OwnedFixture, private_json, wait_for


def anchor(root, binary, tools):
    # This process remains the live session/group leader until its owner has
    # verified cleanup. A stopped/reused process identifier is never targeted.
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    child = subprocess.Popen([sys.executable, str(Path(__file__).with_name('qualify.py')),
                              '--root', str(root), '--binary', str(binary), '--tools', str(tools)],
                             stdin=subprocess.DEVNULL, env=ENVIRONMENT)
    private_json(root / 'pid.json', child.pid)
    while child.poll() is None:
        readable, _, _ = select.select([sys.stdin.buffer], [], [], 0.2)
        if readable:
            # No acknowledgement is valid before the qualifier has exited.
            sys.stdin.buffer.read(1)
            os.killpg(os.getpid(), signal.SIGTERM)
            try:
                child.wait(timeout=20)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=5)
            try:
                OwnedFixture(root).cleanup()
            finally:
                os.killpg(os.getpid(), signal.SIGKILL)
    private_json(root / 'result.json', child.returncode)
    if sys.stdin.buffer.read(1) != b'q':
        try:
            OwnedFixture(root).cleanup()
        finally:
            os.killpg(os.getpid(), signal.SIGKILL)


def check_cleaned(root, state, qualifier_pid, supervisor):
    if any(Path('/proc/' + str(pid)).exists() for pid in [qualifier_pid, *state['children']]):
        raise AssertionError('Owned native qualification child survived cleanup')
    fixture = OwnedFixture(root)
    for unit in state['units']:
        if unit not in (fixture.producer_unit, fixture.observer_unit):
            raise AssertionError('Lifecycle state contains an unowned service')
        state_after = fixture.unit_info(unit)
        if state_after.get('ActiveState') in ('active', 'activating', 'deactivating'):
            raise AssertionError('Owned maintenance service survived cleanup')
        cgroup = Path('/sys/fs/cgroup/system.slice') / unit
        if cgroup.exists() and (cgroup / 'cgroup.procs').read_text().strip():
            raise AssertionError('Owned service cgroup retained tasks')
    if fixture.states.exists() or fixture.private.exists() or fixture.public.exists():
        raise AssertionError('Owned journal or temporary files survived cleanup')
    for port in state['ports']:
        with socket.socket() as probe:
            probe.settimeout(1)
            if probe.connect_ex(('127.0.0.1', port)) == 0:
                raise AssertionError('Owned maintenance qualification listener survived cleanup')
    if supervisor.poll() is not None:
        raise AssertionError('Owned group anchor exited before cleanup verification')


def main(binary, tools, interrupt=False):
    if not sys.platform.startswith('linux') or os.geteuid() != 0:
        raise AssertionError('Resource qualification requires root on an owned disposable Linux host')
    if not binary.is_absolute() or not binary.is_file() or not tools.is_absolute():
        raise AssertionError('Supply absolute existing maintenance qualification binaries')
    if not all((tools / name).is_file() for name in ('prometheus', 'promtool', 'alertmanager', 'amtool')):
        raise AssertionError('Pinned native monitoring tools are missing')
    # DynamicUser mandates PrivateTmp. Keep this exclusive temporary tree out
    # of /tmp rather than bind-mounting private credentials into the observer.
    root = Path('/var/lib') / ('board-maintenance-monitor-' + secrets.token_hex(8))
    root.mkdir(mode=0o700)
    fixture = OwnedFixture(root)
    supervisor = None
    result_path, state_path = root / 'result.json', root / 'state.json'
    try:
        descriptor = os.open(root / 'qualification.log', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as log:
            supervisor = subprocess.Popen([sys.executable, str(Path(__file__)), '--anchor', str(root),
                                            '--binary', str(binary), '--tools', str(tools)],
                                           stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT,
                                           start_new_session=True, env=ENVIRONMENT)

            def healthy():
                if result_path.exists() or supervisor.poll() is not None:
                    raise AssertionError('Native maintenance qualifier exited before healthy authenticated scrape')
                return state_path.exists()

            wait_for(healthy, 60)
            state = json.loads(state_path.read_text())
            qualifier_pid = json.loads((root / 'pid.json').read_text())
            if len(state['children']) != 2 or len(set(state['children'])) != 2:
                raise AssertionError('Native maintenance qualification did not own two monitoring children')
            for pid in [supervisor.pid, qualifier_pid, *state['children']]:
                if os.getpgid(pid) != supervisor.pid or os.getsid(pid) != supervisor.pid:
                    raise AssertionError('Native maintenance child escaped its owned group')
            work = Path(state['work']).resolve()
            if work.parent != fixture.private or not (work / 'pki').is_dir() or not (fixture.private / 'observer.env').is_file():
                raise AssertionError('Owned private maintenance credentials and PKI were not exercised')
            if interrupt:
                os.kill(qualifier_pid, signal.SIGTERM)
            wait_for(result_path.exists, 35 if interrupt else 540)
            if json.loads(result_path.read_text()) != (143 if interrupt else 0):
                raise AssertionError('Maintenance qualification did not complete its expected cleanup path')
            check_cleaned(root, state, qualifier_pid, supervisor)
            supervisor.stdin.write(b'q')
            supervisor.stdin.flush()
            if supervisor.wait(timeout=5) != 0:
                raise AssertionError('Owned maintenance supervisor did not acknowledge successful cleanup')
            print((root / 'qualification.log').read_text(), end='')
            print('PASS ' + ('OS SIGTERM' if interrupt else 'normal stop')
                  + ' removed owned services, child processes, listeners, journals and temporary credentials', flush=True)
    except BaseException:
        # Qualification progress is deliberately static. Never dump native logs,
        # arguments, environment variables or private configuration diagnostics.
        log_path = root / 'qualification.log'
        if log_path.is_file():
            with log_path.open('rb') as log:
                log.seek(max(0, log_path.stat().st_size - 16384))
                for line in log.read(16384).decode(errors='replace').splitlines():
                    if line.startswith(('PASS ', 'STAGE ')):
                        print(line, flush=True)
        raise
    finally:
        if supervisor is not None:
            if supervisor.poll() is None:
                # The retained supervisor still anchors this exact group. First
                # give the qualifier its ordinary unwind path, then bound it.
                os.killpg(supervisor.pid, signal.SIGTERM)
                deadline = time.monotonic() + 20
                while not result_path.exists() and supervisor.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.1)
                if supervisor.poll() is None:
                    os.killpg(supervisor.pid, signal.SIGKILL)
                    supervisor.wait(timeout=5)
            if supervisor.stdin is not None:
                supervisor.stdin.close()
        # This root watcher also owns the exact units/state if an inner process
        # was killed before Python context managers could unwind.
        fixture.cleanup()
        fixture.check_root()
        shutil.rmtree(root)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--tools', type=Path, required=True)
    parser.add_argument('--interrupt', action='store_true')
    parser.add_argument('--anchor', type=Path)
    args = parser.parse_args()
    if args.anchor is not None:
        anchor(args.anchor, args.binary, args.tools)
    else:
        signal.signal(signal.SIGTERM, lambda signum, _frame: sys.exit(128 + signum))
        try:
            main(args.binary, args.tools, args.interrupt)
        except Exception:
            print('Owned maintenance qualification or cleanup failed; private inputs are not logged.', file=sys.stderr)
            raise SystemExit(1) from None
