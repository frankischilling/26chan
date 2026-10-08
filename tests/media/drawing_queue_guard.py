"""A bounded, private PostgreSQL reservation gate for owned idle fixtures only."""
import contextlib
import os
import secrets
import selectors
import subprocess
import time
import urllib.parse

from test_dispatch import PG, SAFE


def database_environment():
    url = urllib.parse.urlparse(os.environ['MIGRATION_DATABASE_URL'])
    return {**SAFE, 'PGHOST': url.hostname, 'PGPORT': str(url.port or 5432),
            'PGUSER': urllib.parse.unquote(url.username),
            'PGPASSWORD': urllib.parse.unquote(url.password),
            'PGDATABASE': urllib.parse.unquote(url.path.lstrip('/'))}


class OwnedQueueGate:
    """Block new reservations while proving there is only one dispatchable job.

    Receipt possession and private quarantine establish this fixture's input;
    this gate prevents a global dispatcher from selecting a competing live job.
    The transaction does not lock media.jobs, which the real dispatcher needs.
    """
    def __init__(self):
        self.process = None
        self.buffer = b''
        self.deadline = None

    def query(self, statement, timeout=12):
        assert self.process is not None and self.process.poll() is None
        assert len(statement) <= 32768 and '\\' not in statement
        marker = 'drawing_gate_' + secrets.token_hex(16)
        self.process.stdin.write((statement + '\n\\echo ' + marker + '\n').encode())
        self.process.stdin.flush()
        deadline = min(time.monotonic() + timeout, self.deadline)
        lines = []
        with selectors.DefaultSelector() as reader:
            reader.register(self.process.stdout, selectors.EVENT_READ)
            while time.monotonic() < deadline:
                while b'\n' in self.buffer:
                    line, self.buffer = self.buffer.split(b'\n', 1)
                    if line.decode('ascii') == marker:
                        return '\n'.join(lines)
                    lines.append(line.decode('ascii'))
                    assert sum(map(len, lines)) <= 4096, 'owned queue gate output exceeded bounds'
                assert self.process.poll() is None, 'owned queue gate rejected its query'
                if not reader.select(min(1, max(0, deadline - time.monotonic()))):
                    continue
                block = os.read(self.process.stdout.fileno(), 4096)
                assert block, 'owned queue gate ended before acknowledgement'
                self.buffer += block
                assert len(self.buffer) <= 8192
        raise AssertionError('owned queue gate timed out')

    def __enter__(self):
        self.deadline = time.monotonic() + 60
        self.process = subprocess.Popen([PG, '-XAtq', '-v', 'ON_ERROR_STOP=1'],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, env=database_environment())
        try:
            assert self.query("BEGIN; SET LOCAL lock_timeout='3s'; SET LOCAL statement_timeout='10s'; SET LOCAL idle_in_transaction_session_timeout='60s'; SELECT singleton FROM media.queue_policy WHERE singleton FOR UPDATE;") == 't'
            return self
        except BaseException:
            self.__exit__(None, None, None)
            raise

    def sole_live_job(self, job):
        # Receivers can become queued without taking the reservation gate again,
        # so queued-only checks are insufficient. Processing jobs can also retry.
        assert self.query("SELECT id FROM media.jobs WHERE state IN ('receiving','queued','processing') ORDER BY id;") == job, 'drawing dispatch requires its sole owned live job'

    def __exit__(self, *_):
        if self.process is None:
            return
        process, self.process = self.process, None
        try:
            if process.poll() is None:
                with contextlib.suppress(BrokenPipeError, OSError):
                    process.stdin.write(b'ROLLBACK;\n\\q\n')
                    process.stdin.flush()
            process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate(timeout=5)
        finally:
            assert process.poll() is not None, 'owned queue gate did not release its connection'


def qualify_owned_queue_gate(fixture):
    """Real PostgreSQL race witness, run only by the owned media supervisor."""
    import hashlib
    from test_dispatch import sql
    assert sql("SELECT count(*) FROM media.jobs WHERE state IN ('receiving','queued','processing');") == '0'
    owned = []
    class InterruptedGate(Exception):
        pass
    try:
        # Actual authenticated reservations create the sentinel rows. They have
        # no uploads, approvals or attachments and stay in the owner's cleanup.
        owned.append(fixture.reserve_http())
        owned.append(fixture.reserve_http())
        try:
            with OwnedQueueGate() as gate:
                assert gate.query("SELECT count(*) FROM media.jobs WHERE state='receiving';") == '2'
                try:
                    gate.sole_live_job(owned[0][0])
                except AssertionError:
                    pass
                else:
                    raise AssertionError('a receiving competitor escaped the drawing dispatch guard')
                blocked = subprocess.run([PG, '-XAtq', '-v', 'ON_ERROR_STOP=1'],
                    input="\\set VERBOSITY sqlstate\nSET ROLE board_media_intake_owner; SET lock_timeout='200ms'; SELECT count(*) FROM media_intake.reserve('drawing-gate-contender.png');\n",
                    text=True, capture_output=True, env=database_environment(), timeout=5)
                assert blocked.returncode != 0 and '55P03' in blocked.stderr, 'reservation did not block on the owned queue gate'
                assert gate.query("SELECT count(*) FROM media.jobs WHERE state IN ('receiving','queued','processing');") == '2'
                raise InterruptedGate()
        except InterruptedGate:
            pass
        # The exceptional path must release the original transaction. A second
        # healthy connection reacquires the same row under the normal timeout.
        with OwnedQueueGate() as gate:
            assert gate.query("SELECT count(*) FROM media.jobs WHERE state='receiving';") == '2'
    finally:
        for job, capability in owned:
            digest = hashlib.sha256(capability.encode('ascii')).hexdigest()
            result = sql(f"""DELETE FROM media.jobs j WHERE j.id='{job}' AND j.filename='synthetic.png'
                AND j.state='receiving' AND j.lease_token IS NULL AND j.attempts=0 AND j.input_bytes IS NULL
                AND EXISTS (SELECT 1 FROM media_intake.handles h WHERE h.job_id=j.id AND h.capability_hash=decode('{digest}','hex'))
                AND NOT EXISTS (SELECT 1 FROM media.assets WHERE job_id=j.id)
                AND NOT EXISTS (SELECT 1 FROM content.post_media WHERE job_id=j.id) RETURNING id;""")
            assert job in result.splitlines(), 'owned queue guard sentinel changed before cleanup'
    print('PASS drawing dispatch guard: real receiving competitors rejected, concurrent reservation blocked, interrupted gate released', flush=True)
