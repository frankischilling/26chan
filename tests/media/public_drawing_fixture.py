"""Receipt/private-input-bound Tegaki qualification with a separate actor witness."""
import os
import json
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
        self.approved = []
        self.reconciled = set()
        self.recovery_baseline = None
        self.completed_modes = set()

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
            AND NOT b.staff_only AND b.oekaki AND {'b.oekaki_replays' if self.mode == 'image-edit' else 'NOT b.oekaki_replays'} AND b.oekaki_width=400 AND b.oekaki_height=400
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

    def approval_source_sql(self, receipt):
        self._validate_owned()
        assert len(self.approved) < (2 if self.mode == 'image-edit' else 1)
        if not self.approved:
            return 'true'
        source_job, source_asset = self.approved[0]
        assert HEX.fullmatch(source_job) and HEX.fullmatch(source_asset)
        assert receipt['target'] == receipt['owner_thread']
        self._ownership_sql(receipt['owner_thread'])
        # A second dispatch is only the edit of this browser's first posted PNG.
        # This is disposable harness evidence, never application provenance.
        return f"""EXISTS (SELECT 1 FROM content.posts p
            JOIN content.boards b ON b.slug=p.board
            JOIN content.post_media m ON m.post_id=p.id
            JOIN media.assets a ON a.id=m.asset_id AND a.job_id=m.job_id
            JOIN post_secrets.posting_history h ON h.post_id=p.id
            WHERE p.board='{self.board}' AND p.thread_id={receipt['owner_thread']}
            AND p.comment='Drawing qualification {self.marker}' AND NOT p.deleted AND NOT b.staff_only
            AND m.job_id='{source_job}' AND m.asset_id='{source_asset}'
            AND NOT m.file_deleted AND m.width=400 AND m.height=400
            AND a.state='approved' AND a.output_format='png' AND h.board=p.board AND h.thread_id=p.thread_id
            AND h.actor_hash=decode('{self._posting_actor_hex()}','hex'))"""

    def approve_receipt(self, marker, job):
        receipt = self._receipt(marker, job)
        assert job not in [item[0] for item in self.approved]
        source = self.approval_source_sql(receipt)
        assert self.input_witness(job) == receipt['input']
        ownership = self._ownership_sql(receipt['owner_thread'])
        # Reservations take this existing singleton lock. Holding it through
        # dispatch prevents new jobs appearing after the all-live-job check.
        # Receiving/processing competitors are also forbidden because they can
        # become queued without reacquiring the reservation lock.
        with OwnedQueueGate() as gate:
            gate.sole_live_job(job)
            assert gate.query(f"""SELECT ({ownership}) AND ({source}) AND EXISTS (SELECT 1 FROM media.jobs j
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
        self.approved.append((job, asset))

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
            self.approved = []
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
                    owner = re.fullmatch(r'DRAWING_OWNER ([a-f0-9]{32}) ([1-9][0-9]{0,18})', text)
                    if owner:
                        assert mode == 'image-edit'
                        self.register_owner(*owner.groups())
                        marker, job = owner.groups()
                    elif receipt:
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
        expected = 2 if mode == 'image-edit' else 1
        assert len(self.approved) == expected
        for index, (job, asset) in enumerate(self.approved):
            comment = ('Drawing edit qualification ' if index else 'Drawing qualification ') + self.marker
            assert sql(f"SELECT count(*) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id JOIN post_secrets.posting_history h ON h.post_id=p.id WHERE p.board='{self.board}' AND p.comment='{comment}' AND m.job_id='{job}' AND m.asset_id='{asset}' AND m.width=400 AND m.height=400 AND m.file_deleted AND NOT p.deleted AND h.actor_hash=decode('{self._posting_actor_hex()}','hex');") == '1'
            assert self.f.http(f'/media/{asset}.png')[0] == 404
        assert sql(f"SELECT count(*) FROM post_secrets.posting_history WHERE board='{self.board}' AND actor_hash=decode('{self._posting_actor_hex()}','hex');") == str(expected + 1)
        assert sql(f"SELECT cardinality(events) FROM post_secrets.public_deletion_actors WHERE actor_hash=decode('{self._actor_hex()}','hex');") == str(expected)
        self.f.finish(self.f.launch([self.f.bin / 'media-publish', 'reconcile', self.f.objects], env=self.f.writer))
        for _, asset in self.approved:
            assert not (self.f.objects / f'{asset}.png').exists()
            assert not (self.f.objects / f'{asset}.thumb.png').exists()
            assert sql(f"SELECT NOT EXISTS (SELECT 1 FROM media.assets WHERE id='{asset}');") == 't'
        # Record retirement only after the real reconciler removed both outputs
        # and approval metadata for every already-verified owned attachment.
        self.reconciled.update(self.approved)
        remaining = private_inputs(self.f.root, self.f.quarantine, self.f.intake_user.pw_uid) - self.recovery_baseline
        assert all(name.removesuffix('.input') in self.f.ids for name in remaining), 'unregistered completed drawing intake remains'
        self.recovery_baseline = None
        self.completed_modes.add(mode)
        self.browser = None
        print(f'PASS drawing {mode}: real pointer canvas -> exact receipt -> isolated decoder -> persisted 400x400 PNG -> owned deletion', flush=True)


class PublicDrawingEdit(PublicDrawingUpload):
    """Own an empty disposable /i/ board during its source-specific browser case."""
    _changed = ('title', 'description', 'posting_reply_seconds', 'posting_image_seconds',
                'posting_thread_seconds', 'deletion_known_min_seconds', 'deletion_unknown_min_seconds')

    def __init__(self, host):
        super().__init__(host)
        self.board = self._owned_board = 'i'
        self.mode = 'image-edit'
        self.lease = None
        self.lease_phase = None
        self.owner_thread = None

    @staticmethod
    def _json_sql(value):
        # The SQL transactions set standard_conforming_strings before using this
        # literal. Values are saved public policy rows, never service credentials.
        return "'" + json.dumps(value, sort_keys=True, separators=(',', ':')).replace("'", "''") + "'::jsonb"

    def create_board(self):
        assert not self.created and self.lease is None
        assert self.board == self._owned_board == 'i' and re.fullmatch('[a-f0-9]{32}', self.marker)
        before = json.loads(sql("SELECT to_jsonb(b) FROM content.boards b WHERE slug='i';"))
        active = {**before, 'title': 'Drawing qualification', 'description': self.marker,
                  **{name: 0 for name in self._changed[2:]}}
        # Save the exact recovery data BEFORE the mutation. An ambiguous psql
        # result after COMMIT must still leave cleanup able to identify the lease.
        self.lease = dict(before=before,active=active)
        self.created = True
        self.lease_phase = 'prepared'
        self._validate_owned()
        original = self._json_sql(before)
        expected = self._json_sql(active)
        sql(f"""BEGIN; SET LOCAL standard_conforming_strings=on; SET LOCAL lock_timeout='1s';
            DO $owned_edit$ BEGIN
                PERFORM 1 FROM content.boards WHERE slug='i' FOR UPDATE NOWAIT;
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='i') IS DISTINCT FROM {original}
                    OR EXISTS (SELECT 1 FROM content.posts WHERE board='i')
                    OR EXISTS (SELECT 1 FROM content.threads WHERE board='i')
                    OR EXISTS (SELECT 1 FROM post_secrets.posting_history WHERE board='i')
                    OR EXISTS (SELECT 1 FROM post_secrets.posting_thread_actions WHERE board='i')
                THEN RAISE EXCEPTION 'Drawing Edit requires an untouched disposable source board'; END IF;
                UPDATE content.boards SET title='Drawing qualification',description='{self.marker}',
                    posting_reply_seconds=0,posting_image_seconds=0,posting_thread_seconds=0,
                    deletion_known_min_seconds=0,deletion_unknown_min_seconds=0 WHERE slug='i';
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='i') IS DISTINCT FROM {expected}
                THEN RAISE EXCEPTION 'Drawing Edit changed unrelated policy'; END IF;
            END $owned_edit$; COMMIT;""")
        self.lease_phase = 'active'

    def _validate_owned(self):
        assert self.created and self.board == self._owned_board == 'i' and self.mode == 'image-edit'
        assert re.fullmatch('[a-f0-9]{32}', self.marker)
        assert type(self.lease) is dict and set(self.lease) == {'before', 'active'}
        before, active = self.lease['before'], self.lease['active']
        assert before['slug'] == active['slug'] == 'i'
        assert before['title'] == 'Oekaki' and active['title'] == 'Drawing qualification'
        assert active['description'] == self.marker
        assert {key: value for key, value in before.items() if key not in self._changed} == {
            key: value for key, value in active.items() if key not in self._changed}
        assert active['oekaki'] is True and active['oekaki_replays'] is True
        assert active['staff_only'] is False and active['text_only'] is False
        assert active['oekaki_width'] == active['oekaki_height'] == 400 and active['image_limit'] > 0
        assert all(active[name] == 0 for name in self._changed[2:])

    def _policy_guard(self):
        self._validate_owned()
        return f"EXISTS (SELECT 1 FROM content.boards b WHERE slug='i' AND to_jsonb(b)={self._json_sql(self.lease['active'])})"

    def _ownership_sql(self, owner_thread):
        return f"({super()._ownership_sql(owner_thread)}) AND ({self._policy_guard()})"

    def register_owner(self, marker, owner_thread):
        self._validate_owned()
        assert marker == self.marker and self.owner_thread is None
        ownership = self._ownership_sql(owner_thread)
        assert sql(f"SELECT ({ownership}) AND (SELECT count(*) FROM content.posts WHERE board='i')=1 AND (SELECT count(*) FROM content.threads WHERE board='i')=1;") == 't'
        self.owner_thread = owner_thread

    def register_receipt(self, marker, job, capability_hash, target, owner_thread):
        assert self.owner_thread is not None and owner_thread == target == self.owner_thread
        super().register_receipt(marker, job, capability_hash, target, owner_thread)

    def reset_deletion_quota(self):
        guard = self._policy_guard()
        sql(f"""BEGIN; SET LOCAL standard_conforming_strings=on;
            DO $owned_edit$ BEGIN
                PERFORM 1 FROM content.boards WHERE slug='i' FOR UPDATE NOWAIT;
                IF NOT ({guard}) THEN RAISE EXCEPTION 'Drawing Edit policy ownership changed'; END IF;
                DELETE FROM post_secrets.public_deletion_actors WHERE actor_hash=decode('{self._actor_hex()}','hex');
            END $owned_edit$; COMMIT;""")

    def cleanup_board(self):
        guard = self._policy_guard()
        original = self._json_sql(self.lease['before'])
        if self.lease_phase == 'prepared' and self.owner_thread is None and not self.receipts and not self.approved:
            if json.loads(sql("SELECT to_jsonb(b) FROM content.boards b WHERE slug='i';")) == self.lease['before']:
                # The guarded mutation never took effect. Leave all later work
                # untouched, including rows that caused its initial refusal.
                self.created = False
                return
        owner = self.owner_thread or '0'
        assert re.fullmatch('0|[1-9][0-9]{0,18}', owner) and int(owner) <= 9223372036854775807
        approved = []
        assert self.reconciled <= set(self.approved)
        for index, (job, asset) in enumerate(self.approved):
            assert index < 2 and HEX.fullmatch(job) and HEX.fullmatch(asset) and job in self.receipts
            assert not self.receipts[job]['revoked'] and self.receipts[job]['owner_thread'] == owner
            comment = ('Drawing edit qualification ' if index else 'Drawing qualification ') + self.marker
            proof = "EXISTS (SELECT 1 FROM media.assets a WHERE a.id=m.asset_id AND a.job_id=m.job_id AND a.state='approved' AND a.output_format='png')"
            if (job, asset) in self.reconciled:
                proof = f"({proof} OR (m.file_deleted AND NOT EXISTS (SELECT 1 FROM media.assets a WHERE a.id=m.asset_id)))"
            approved.append(f"(m.job_id='{job}' AND m.asset_id='{asset}' AND p.comment='{comment}' AND {proof})")
        attachment = ' OR '.join(approved) or 'false'
        assignments = ','.join(f"{name}=original.{name}" for name in self._changed)
        # Keep all ownership checks, deletion and policy restoration under the
        # board lock. A foreign row or changed policy leaves all evidence intact.
        sql(f"""BEGIN; SET LOCAL standard_conforming_strings=on; SET LOCAL lock_timeout='1s';
            DO $owned_edit$ DECLARE original content.boards; BEGIN
                PERFORM 1 FROM content.boards WHERE slug='i' FOR UPDATE NOWAIT;
                IF NOT ({guard}) THEN RAISE EXCEPTION 'Drawing Edit policy ownership changed'; END IF;
                IF EXISTS (SELECT 1 FROM content.posts p WHERE p.board='i' AND
                    (p.deleted OR p.thread_id<>{owner}
                    OR NOT EXISTS (SELECT 1 FROM post_secrets.posting_history h WHERE h.post_id=p.id
                        AND h.board=p.board AND h.thread_id=p.thread_id
                        AND h.actor_hash=decode('{self._posting_actor_hex()}','hex'))
                    OR NOT ((p.id={owner} AND p.comment='Drawing ownership {self.marker}'
                        AND NOT EXISTS (SELECT 1 FROM content.post_media m WHERE m.post_id=p.id))
                        OR (p.id<>{owner} AND EXISTS (SELECT 1 FROM content.post_media m
                            WHERE m.post_id=p.id AND m.width=400 AND m.height=400
                                AND ({attachment}))))))
                    OR EXISTS (SELECT 1 FROM content.threads t WHERE t.board='i' AND (t.id<>{owner}
                        OR NOT EXISTS (SELECT 1 FROM content.posts p WHERE p.board='i' AND p.id=t.id AND p.thread_id=t.id)))
                    OR EXISTS (SELECT 1 FROM post_secrets.posting_history h WHERE h.board='i'
                        AND (h.actor_hash<>decode('{self._posting_actor_hex()}','hex')
                            OR NOT EXISTS (SELECT 1 FROM content.posts p WHERE p.id=h.post_id AND p.board='i')))
                    OR EXISTS (SELECT 1 FROM post_secrets.posting_thread_actions WHERE board='i'
                        AND actor_hash<>decode('{self._posting_actor_hex()}','hex'))
                    OR EXISTS (SELECT 1 FROM content.reports r JOIN content.posts p ON p.id=r.post_id WHERE p.board='i')
                THEN RAISE EXCEPTION 'Drawing Edit found unowned posts or attachments'; END IF;
                DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board='i');
                DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board='i');
                DELETE FROM content.posts WHERE board='i';
                DELETE FROM content.threads WHERE board='i';
                DELETE FROM post_secrets.posting_history WHERE board='i' AND actor_hash=decode('{self._posting_actor_hex()}','hex');
                DELETE FROM post_secrets.posting_thread_actions WHERE board='i' AND actor_hash=decode('{self._posting_actor_hex()}','hex');
                DELETE FROM post_secrets.public_deletion_actors WHERE actor_hash=decode('{self._actor_hex()}','hex');
                SELECT * INTO original FROM jsonb_populate_record(NULL::content.boards,{original});
                UPDATE content.boards SET {assignments} WHERE slug='i';
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='i') IS DISTINCT FROM {original}
                THEN RAISE EXCEPTION 'Drawing Edit did not restore source policy'; END IF;
            END $owned_edit$; COMMIT;""")
        self.created = False

    def exercise(self):
        self.create_board()
        self.reset_deletion_quota()
        self.run_browser('image-edit')
