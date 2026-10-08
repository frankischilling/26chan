"""Receipt/private-input-bound Tegaki qualification with a separate actor witness."""
import os
import pwd
import re
import secrets
import selectors
import subprocess
import time

from drawing_input_witness import private_input, private_inputs
from drawing_queue_guard import OwnedQueueGate, qualify_owned_queue_gate
from public_upload_fixture import PublicUpload, finish_browser
from test_dispatch import HEX, REPO, SAFE, sql


class PublicDrawingUpload(PublicUpload):
    def __init__(self, host):
        super().__init__(host.f)
        # Reuse the running public HTTP service, never start/stop its unit here.
        self.origin = host.origin
        self._poster_id_key = host._poster_id_key
        self.marker = secrets.token_hex(16)
        self.receipts = {}
        self.mode = None
        self.approved = None
        self.recovery_baseline = None

    def create_board(self):
        super().create_board()
        assert re.fullmatch('[a-f0-9]{32}', self.marker)
        changed = sql(f"UPDATE content.boards SET title='Drawing qualification',description='{self.marker}',oekaki=true,oekaki_replays=false,oekaki_width=400,oekaki_height=400 WHERE slug='{self.board}' AND title='Upload qualification' RETURNING slug;")
        assert self.board in changed.splitlines()

    def _validate_owned(self):
        assert self.created and self.board == self._owned_board
        assert re.fullmatch(r'u[0-9a-f]{8}', self.board)
        assert re.fullmatch('[a-f0-9]{32}', self.marker)
        assert self.mode in ('ordinary', 'quick-reply')

    def _ownership_sql(self, owner_thread):
        self._validate_owned()
        assert re.fullmatch('[1-9][0-9]{0,18}', owner_thread)
        assert int(owner_thread) <= 9223372036854775807
        return f"""EXISTS (SELECT 1 FROM content.boards b
            JOIN content.posts p ON p.board=b.slug
            JOIN post_secrets.posting_history h ON h.board=p.board AND h.post_id=p.id
            WHERE b.slug='{self.board}' AND b.title='Drawing qualification' AND b.description='{self.marker}'
            AND b.oekaki AND NOT b.oekaki_replays AND b.oekaki_width=400 AND b.oekaki_height=400
            AND p.id={owner_thread} AND p.thread_id=p.id AND NOT p.deleted
            AND p.comment='Drawing ownership {self.marker}' AND h.thread_id=p.id
            AND h.actor_hash=decode('{self._posting_actor_hex()}','hex'))"""

    def input_witness(self, job):
        return private_input(self.f.root, self.f.quarantine, self.f.intake_user.pw_uid, job)

    def register_receipt(self, marker, job, capability_hash, target, owner_thread):
        self._validate_owned()
        assert marker == self.marker and HEX.fullmatch(job) and job not in self.receipts
        assert re.fullmatch('[a-f0-9]{64}', capability_hash)
        assert target == ('0' if self.mode == 'ordinary' else owner_thread)
        ownership = self._ownership_sql(owner_thread)
        witness = self.input_witness(job)
        # There is no job-to-board or job-to-actor relation in the schema.
        # These are separate witnesses: owner thread, receipt, and this run's
        # private intake input. Never infer ownership from filename alone.
        assert sql(f"""SELECT ({ownership}) AND EXISTS (SELECT 1 FROM media.jobs j
            JOIN media_intake.handles h ON h.job_id=j.id WHERE j.id='{job}' AND j.filename='tegaki.png'
            AND j.state='queued' AND j.lease_token IS NULL AND j.attempts=0 AND j.input_bytes={witness[2]}
            AND h.capability_hash=decode('{capability_hash}','hex')) AND NOT EXISTS
            (SELECT 1 FROM content.post_media WHERE job_id='{job}');""") == 't'
        self.receipts[job] = dict(marker=marker, target=target, owner_thread=owner_thread,
                                  capability_hash=capability_hash, input=witness, revoked=False)
        self.f.ids.append(job)

    def _receipt(self, marker, job):
        self._validate_owned()
        assert marker == self.marker and HEX.fullmatch(job)
        receipt = self.receipts.get(job)
        assert receipt and receipt['marker'] == marker and not receipt['revoked']
        assert receipt['target'] == ('0' if self.mode == 'ordinary' else receipt['owner_thread'])
        return receipt

    def remove_revoked_queued(self, marker, job):
        receipt = self._receipt(marker, job)
        ownership = self._ownership_sql(receipt['owner_thread'])
        assert self.input_witness(job) == receipt['input']
        # Runtime cancellation only revokes the handle. This checked transaction
        # is disposable-harness cleanup, never evidence of runtime job removal.
        # Keep the verified job ID in f.ids for the owned quarantine cleanup.
        result = sql(f"""BEGIN;
            DO $owned_drawing$ BEGIN
              IF NOT ({ownership}) THEN RAISE EXCEPTION 'Drawing owner changed'; END IF;
              PERFORM 1 FROM media.jobs WHERE id='{job}' AND filename='tegaki.png'
                AND state='queued' AND lease_token IS NULL AND attempts=0 FOR UPDATE;
              IF NOT FOUND THEN RAISE EXCEPTION 'Drawing job is not an unleased queued job'; END IF;
              DELETE FROM media.jobs j WHERE j.id='{job}' AND j.state='queued' AND j.lease_token IS NULL AND j.attempts=0
                AND NOT EXISTS (SELECT 1 FROM media_intake.handles WHERE job_id=j.id)
                AND NOT EXISTS (SELECT 1 FROM content.post_media WHERE job_id=j.id)
                AND NOT EXISTS (SELECT 1 FROM media.assets WHERE job_id=j.id);
              IF NOT FOUND THEN RAISE EXCEPTION 'Drawing cancellation was not proven'; END IF;
            END $owned_drawing$;
            COMMIT;
            SELECT count(*) FROM media.jobs WHERE id='{job}';""")
        assert result.splitlines()[-1] == '0'
        receipt['revoked'] = True

    def approve_receipt(self, marker, job):
        receipt = self._receipt(marker, job)
        assert self.approved is None
        assert self.input_witness(job) == receipt['input']
        ownership = self._ownership_sql(receipt['owner_thread'])
        # Reservations take this existing singleton lock. Holding it through
        # dispatch prevents new jobs appearing after the all-live-job check.
        # Receiving/processing competitors are also forbidden because they can
        # become queued without reacquiring the reservation lock.
        with OwnedQueueGate() as gate:
            gate.sole_live_job(job)
            assert gate.query(f"""SELECT ({ownership}) AND EXISTS (SELECT 1 FROM media.jobs j
                JOIN media_intake.handles h ON h.job_id=j.id WHERE j.id='{job}' AND j.state='queued'
                AND j.lease_token IS NULL AND j.attempts=0 AND j.filename='tegaki.png'
                AND j.input_bytes={receipt['input'][2]} AND j.expires_at>clock_timestamp()
                AND h.capability_hash=decode('{receipt['capability_hash']}','hex'));""") == 't'
            dispatch = self.f.dispatch()
            try:
                asset = self.f.finish(dispatch).decode().strip()
                assert HEX.fullmatch(asset)
                assert gate.query(f"SELECT job_id FROM media.assets WHERE id='{asset}' AND state='approved';") == job
            finally:
                # A timed-out command must not outlive the reservation gate and
                # claim a newly admitted job after this fixture has failed.
                if dispatch.poll() is None:
                    try:
                        self.f.stop(dispatch)
                    except subprocess.TimeoutExpired:
                        dispatch.kill()
                        dispatch.communicate(timeout=5)
                assert dispatch.poll() is not None, 'drawing dispatcher outlived its gate'
        self.f.clean_vm()
        self.approved = (job, asset)

    def recover_unregistered_inputs(self):
        if self.recovery_baseline is None:
            return
        current = private_inputs(self.f.root, self.f.quarantine, self.f.intake_user.pw_uid)
        for name in sorted(current - self.recovery_baseline):
            job = name.removesuffix('.input')
            if job in self.f.ids:
                continue
            witness = self.input_witness(job)
            # A completed intake before the RECEIPT pipe message still leaves
            # this unique private input. Recover only exact unleased jobs with
            # matching committed bytes (or a receiving row before its size commit)
            # and no approval/attachment; never search globally
            # by tegaki.png. Parent cleanup removes these owned rows and files.
            assert sql(f"""SELECT EXISTS (SELECT 1 FROM media.jobs j WHERE j.id='{job}'
                AND j.filename='tegaki.png' AND j.lease_token IS NULL AND j.attempts=0
                AND ((j.state='queued' AND j.input_bytes={witness[2]})
                  OR (j.state='receiving' AND j.input_bytes IS NULL)))
                AND NOT EXISTS (SELECT 1 FROM content.post_media WHERE job_id='{job}')
                AND NOT EXISTS (SELECT 1 FROM media.assets WHERE job_id='{job}');""") == 't'
            self.f.ids.append(job)
        self.recovery_baseline = None

    def cleanup(self):
        try:
            if self.browser is not None:
                self.f.stop(self.browser)
                self.browser = None
            if self.recovery_baseline is not None:
                # Stop this run's intake before taking the recovery snapshot.
                # A completed file can precede its finish_upload DB commit.
                self.f.stop(self.f.intake_unit)
                self.recover_unregistered_inputs()
            super().cleanup()
        except BaseException as failure:
            # Refusing suspect input must not strand public/intake/VM processes.
            # Retain the private workspace and unproven database evidence; make
            # a bounded shutdown attempt for every process this run owns before
            # surfacing the failure to the outer protected cleanup.
            stopped = True
            for process in reversed(self.f.processes):
                try:
                    self.f.stop(process)
                except BaseException:
                    stopped = False
            broker = getattr(self.f, 'broker_dir', None)
            if broker is not None and (broker / 'requests').exists():
                try:
                    self.f.clean_vm()
                except BaseException:
                    stopped = False
            message = ('Drawing cleanup refused suspect evidence; owned processes stopped and evidence retained'
                       if stopped else 'Drawing cleanup refused suspect evidence; bounded shutdown attempted for every owned process, inspect retained evidence')
            raise RuntimeError(message) from failure

    def exercise(self):
        qualify_owned_queue_gate(self.f)
        self.create_board()
        for mode in ('ordinary', 'quick-reply'):
            self.mode = mode
            self.reset_deletion_quota()
            self.reset_posting_history()
            self.approved = None
            self.run_browser(mode)

    def run_browser(self, mode):
        self._validate_owned()
        browser_user = pwd.getpwuid(REPO.stat().st_uid)
        assert browser_user.pw_uid != 0
        node = os.environ['PUBLIC_UPLOAD_NODE']
        assert os.path.isabs(node) and os.path.isfile(node)
        self.recovery_baseline = private_inputs(self.f.root, self.f.quarantine, self.f.intake_user.pw_uid)
        script = 'drawing-upload.mjs'
        process = subprocess.Popen([node, str(REPO / 'tests/browser' / script), self.origin,
                                    self.board, self.marker, mode],
                                   env={**SAFE, 'HOME': browser_user.pw_dir}, user=browser_user.pw_uid,
                                   group=browser_user.pw_gid, extra_groups=[], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        self.f.processes.append(process)
        self.browser = process
        # All protocol lines are bounded and stay in this private pipe. Neither
        # raw capabilities nor arbitrary browser error bodies are printed.
        pending = b''
        deadline = time.monotonic() + 150
        complete = False
        with selectors.DefaultSelector() as reader:
            reader.register(process.stdout, selectors.EVENT_READ)
            while not complete:
                assert time.monotonic() < deadline, 'drawing browser deadline exceeded'
                events = reader.select(min(1, max(0, deadline - time.monotonic())))
                if not events:
                    if process.poll() is not None:
                        finish_browser(process, script)
                        raise AssertionError('drawing browser ended without completion')
                    continue
                block = os.read(process.stdout.fileno(), 1024)
                if not block:
                    finish_browser(process, script)
                    raise AssertionError('drawing browser ended without completion')
                pending += block
                assert len(pending) <= 2048, 'drawing browser protocol overflow'
                while b'\n' in pending:
                    line, pending = pending.split(b'\n', 1)
                    assert len(line) <= 512
                    text = line.decode('ascii')
                    receipt = re.fullmatch(r'DRAWING_RECEIPT ([a-f0-9]{32}) ([a-f0-9]{32}) ([a-f0-9]{64}) (0|[1-9][0-9]{0,18}) ([1-9][0-9]{0,18})', text)
                    action = re.fullmatch(r'DRAWING_(REVOKED|APPROVE) ([a-f0-9]{32}) ([a-f0-9]{32})', text)
                    if receipt:
                        self.register_receipt(*receipt.groups())
                        marker, job = receipt.group(1, 2)
                    elif action:
                        operation, marker, job = action.groups()
                        (self.remove_revoked_queued if operation == 'REVOKED' else self.approve_receipt)(marker, job)
                    else:
                        assert text.startswith(f'PASS drawing {mode}: real Tegaki pointer strokes,')
                        complete = True
                        break
                    process.stdin.write(f'ACK {marker} {job}\n'.encode('ascii'))
                    process.stdin.flush()
        process.stdin.close()
        process.stdin = None
        finish_browser(process, script)
        assert self.approved is not None
        job, asset = self.approved
        assert sql(f"SELECT count(*) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id JOIN post_secrets.posting_history h ON h.post_id=p.id WHERE p.board='{self.board}' AND p.comment='Drawing qualification {self.marker}' AND m.job_id='{job}' AND m.asset_id='{asset}' AND m.width=400 AND m.height=400 AND m.file_deleted AND NOT p.deleted AND h.actor_hash=decode('{self._posting_actor_hex()}','hex');") == '1'
        assert sql(f"SELECT count(*) FROM post_secrets.posting_history WHERE board='{self.board}' AND actor_hash=decode('{self._posting_actor_hex()}','hex');") == '2'
        assert sql(f"SELECT cardinality(events) FROM post_secrets.public_deletion_actors WHERE actor_hash=decode('{self._actor_hex()}','hex');") == '1'
        assert self.f.http(f'/media/{asset}.png')[0] == 404
        self.f.finish(self.f.launch([self.f.bin / 'media-publish', 'reconcile', self.f.objects], env=self.f.writer))
        assert not (self.f.objects / f'{asset}.png').exists()
        assert not (self.f.objects / f'{asset}.thumb.png').exists()
        remaining = private_inputs(self.f.root, self.f.quarantine, self.f.intake_user.pw_uid) - self.recovery_baseline
        assert all(name.removesuffix('.input') in self.f.ids for name in remaining), 'unregistered completed drawing intake remains'
        self.recovery_baseline = None
        print(f'PASS drawing {mode}: real pointer canvas -> exact receipt -> isolated decoder -> persisted 400x400 PNG -> owned deletion', flush=True)
