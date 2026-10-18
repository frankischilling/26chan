#!/usr/bin/env python3
"""Owned synthetic upgrade database only; exercises the actual operator binary."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys

binary = str(Path(sys.argv[1]).resolve())
psql = os.environ.get("ERASURE_TEST_PSQL", "psql")
host = os.environ["ERASURE_TEST_HOST"]
port = os.environ["ERASURE_TEST_PORT"]
database = os.environ["ERASURE_TEST_DATABASE"]
owner = os.environ["ERASURE_TEST_OWNER"]
cluster = os.environ["BOARD_TEST_CLUSTER"]
assert re.fullmatch(r"/tmp/board-(?:postgres|historical-erasure)\.[A-Za-z0-9]{8}", cluster)
assert Path(cluster).is_dir() and not Path(cluster).is_symlink()
assert database in ("imageboard", "historical_erasure", "historical_restore")

def sql(statement, role=None, ok=True):
    result = subprocess.run([psql, "-XAt", "-v", "ON_ERROR_STOP=1", "-h", host,
        "-p", port, "-U", role or owner, "-d", database, "-c", statement],
        text=True, capture_output=True, timeout=20)
    if ok:
        assert result.returncode == 0, result.stderr
    else:
        assert result.returncode != 0, "Expected a database refusal"
    return result.stdout.strip()

actual = sql("SHOW data_directory")
assert actual in (cluster, cluster + "/data")
assert sql("SELECT current_database()") == database

def command(*args, role="board_migrator", ok=True):
    env = dict(os.environ)
    from urllib.parse import quote
    env["MIGRATION_DATABASE_URL"] = (
        f"postgresql://{role}@localhost:{port}/{database}?host={quote(host,safe='')}"
    )
    result = subprocess.run([binary, *args], env=env, text=True,
        capture_output=True, timeout=40)
    if ok:
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)
    assert result.returncode != 0, "Expected an operator refusal"
    assert not result.stdout.strip(), "Refusal emitted a successful result"
    return result.stderr

def preview(board="hclean"):
    return command("preview", board)

def apply(plan, **kwargs):
    return command("apply", plan["board"], "--confirm-permanent-erasure",
        plan["receipt"], "--writers-stopped", **kwargs)

def rows():
    return sql("""SELECT encode(sha256(convert_to(coalesce(jsonb_agg(
        jsonb_build_array(relation,value) ORDER BY relation,value)::text,'[]'),'UTF8')),'hex')
        FROM public.historical_rows()""")

sql("""CREATE FUNCTION public.historical_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private')
 AND c.relkind='r' LOOP
 RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.historical_rows() FROM PUBLIC;""")
# One legacy membership and one immutable evidence row per owned report.
sql("""BEGIN; SET LOCAL ROLE board_report_admission_owner;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
SELECT r.id,sha256(convert_to(r.id::text,'UTF8')),r.board,r.post_id,p.thread_id,'2020-01-01 UTC'
FROM content.reports r JOIN content.posts p ON p.id=r.post_id AND p.board=r.board
WHERE r.id BETWEEN 31010001 AND 31010072;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,evaluated_at)
SELECT id,1,'2020-01-01 UTC' FROM content.reports WHERE id BETWEEN 31010001 AND 31010072;
COMMIT;""")
# Synthetic retained captures test FK cleanup and capacity accounting. Their
# data is fixture-only; no claim is made about a past real admission snapshot.
sql("""BEGIN;
INSERT INTO post_secrets.report_post_snapshots(report_id,board,post_id,version,captured_at,payload,charged_bytes)
SELECT r.id,r.board,r.post_id,1,'2020-01-01 UTC',v.payload,octet_length(v.payload::text)+128
FROM content.reports r CROSS JOIN LATERAL
 (SELECT jsonb_build_object('board',r.board,'post_id',r.post_id,'comment','Owned retained capture') payload) v
WHERE r.id BETWEEN 31010001 AND 31010072;
UPDATE post_secrets.report_snapshot_capacity SET used_bytes=(SELECT sum(charged_bytes) FROM post_secrets.report_post_snapshots);
COMMIT;""")

baseline = rows()
# The operator must use its existing SET-only owner membership for the report
# restriction seal. Do not broaden the migrator's direct function grants.
assert sql("SELECT has_function_privilege('board_migrator','content.restrictions_ready()','EXECUTE')") == "f"
sql("SELECT content.restrictions_ready()", role="board_migrator", ok=False)
assert sql("SELECT has_function_privilege('board_restriction_owner','content.report_profile_ready()','EXECUTE')") == "f"
assert sql("SELECT content.report_profile_ready()", role="board_migrator") == "t"
sql("BEGIN; SET LOCAL ROLE board_restriction_owner; SELECT content.report_profile_ready()", role="board_migrator", ok=False)
first = preview()
assert rows() == baseline, "Preview changed persisted rows"
assert len(first["report_ids"]) == 64 and len(first["post_ids"]) == 64 and first["thread_ids"] == []
assert 31000001 not in first["post_ids"] and not first["applied"]
for role in (owner, "board_public", "board_staff", "board_auth", "board_media", "board_media_read", "board_media_intake", "board_monitor"):
    command("preview", "hclean", role=role, ok=False)
    apply(first, role=role, ok=False)
assert rows() == baseline, "Runtime refusal changed rows"

# A reviewed batch cannot be used for another board or after its saved row changes.
wrong = dict(first, board="hother")
apply(wrong, ok=False)
sql("UPDATE content.reports SET reason='Changed owned report' WHERE id=31010001")
changed = rows()
apply(first, ok=False)
assert rows() == changed
first = preview()

# Prove stale-state rejection after an observed board-lock wait, not a sleep
# that merely hopes the operator arrived before an unrelated write.
import select
import time
from urllib.parse import quote
holder = subprocess.Popen([psql, "-XAt", "-v", "ON_ERROR_STOP=1", "-h", host,
    "-p", port, "-U", "board_migrator", "-d", database],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
waiting = None
try:
    holder.stdin.write("BEGIN; SELECT slug FROM content.boards WHERE slug='hclean' FOR UPDATE;\n")
    holder.stdin.flush()
    deadline = time.monotonic() + 5
    observed = False
    while time.monotonic() < deadline:
        if select.select([holder.stdout], [], [], 0.1)[0]:
            if holder.stdout.readline().strip() == "hclean":
                observed = True
                break
    assert observed, "Holder never confirmed its board lock"
    env = dict(os.environ)
    env["MIGRATION_DATABASE_URL"] = f"postgresql://board_migrator@localhost:{port}/{database}?host={quote(host,safe='')}"
    waiting = subprocess.Popen([binary, "apply", "hclean", "--confirm-permanent-erasure",
        first["receipt"], "--writers-stopped"], env=env, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    deadline = time.monotonic() + 1.5
    observed = False
    while time.monotonic() < deadline:
        if sql("""SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database()
            AND usename='board_migrator' AND wait_event_type='Lock'
            AND query LIKE 'SELECT slug FROM content.boards WHERE slug=$1%')""") == "t":
            observed = True
            break
        time.sleep(0.02)
    assert observed, "Operator did not reach the expected board-lock wait"
    holder.stdin.write("UPDATE content.reports SET reason='Changed while operator waited' WHERE id=31010001; COMMIT;\n")
    holder.stdin.close()
    assert holder.wait(timeout=5) == 0, holder.stderr.read()
    out, error = waiting.communicate(timeout=10)
    assert waiting.returncode != 0 and not out.strip() and "preview changed" in error, error
finally:
    if holder.poll() is None:
        holder.terminate()
        holder.wait(timeout=5)
    if waiting is not None and waiting.poll() is None:
        waiting.terminate()
        waiting.wait(timeout=5)
changed = rows()
first = preview()
assert len(first["report_ids"]) == 64 and len(first["post_ids"]) == 64

# Refuse a parent sweep until all of its historical children are erased.
sql("BEGIN; SELECT slug FROM content.boards WHERE slug='hclean' FOR UPDATE; UPDATE content.threads SET deleted=true,content_erased=true WHERE id=31000001; COMMIT",
    role="board_migrator", ok=False)
assert rows() == changed

# Independent catalog defects fail before any report or content removal.
for change, restore in (
    ("ALTER FUNCTION content.report_profile_ready() SECURITY INVOKER",
     "ALTER FUNCTION content.report_profile_ready() SECURITY DEFINER"),
    ("ALTER FUNCTION post_secrets.guard_post_erasure() SECURITY INVOKER",
     "ALTER FUNCTION post_secrets.guard_post_erasure() SECURITY DEFINER"),
    ("ALTER FUNCTION post_secrets.guard_thread_erasure() SET search_path=public",
     "ALTER FUNCTION post_secrets.guard_thread_erasure() SET search_path=pg_catalog,pg_temp"),
    ("ALTER TABLE post_secrets.report_post_snapshots DISABLE TRIGGER release_report_snapshot",
     "ALTER TABLE post_secrets.report_post_snapshots ENABLE TRIGGER release_report_snapshot"),
    ("ALTER FUNCTION content.lock_report_network_policy() SECURITY INVOKER",
     "ALTER FUNCTION content.lock_report_network_policy() SECURITY DEFINER"),
):
    sql(change)
    drift = rows()
    apply(first, ok=False)
    assert rows() == drift, "Catalog refusal changed rows"
    sql(restore)
assert rows() == changed
sql("CREATE TABLE public.unreviewed_report_child(id bigint PRIMARY KEY REFERENCES content.reports(id) ON DELETE CASCADE); INSERT INTO public.unreviewed_report_child VALUES(31010001)")
apply(first, ok=False)
assert sql("SELECT count(*) FROM public.unreviewed_report_child") == "1"
assert rows() == changed
sql("DROP TABLE public.unreviewed_report_child")

# A deferred failure runs at commit after report cascades and post erasure.
# The report-profile seal now covers every report trigger. First prove that
# this owned test trigger is rejected as drift. Then attest only this disposable
# fixture so the same sentinel can exercise the later rollback boundary.
profile_seal = sql("SELECT encode(definition_hash,'hex') FROM post_secrets.report_profile_catalog WHERE singleton")
sql("""CREATE FUNCTION public.refuse_owned_erasure_commit() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN IF OLD.id=31010003 THEN RAISE EXCEPTION 'Owned rollback sentinel'; END IF; RETURN OLD; END $$;
CREATE CONSTRAINT TRIGGER owned_erasure_failure AFTER DELETE ON content.reports
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.refuse_owned_erasure_commit();""")
error = apply(first, ok=False)
assert "schema or visibility is unavailable" in error, error
assert rows() == changed, "Unattested test trigger changed persisted rows"
sql("UPDATE post_secrets.report_profile_catalog SET definition_hash=post_secrets.report_profile_catalog_hash() WHERE singleton")
assert sql("SELECT content.report_profile_ready()", role="board_migrator") == "t"
instrumented = rows()
error = apply(first, ok=False)
assert "Cannot confirm historical cleanup" in error, error
assert rows() == instrumented, "Deferred failure did not roll back payload and report cascades"
sql("DROP TRIGGER owned_erasure_failure ON content.reports; DROP FUNCTION public.refuse_owned_erasure_commit()")
assert re.fullmatch(r"[0-9a-f]{64}", profile_seal)
sql("UPDATE post_secrets.report_profile_catalog SET definition_hash=decode('" + profile_seal + "','hex') WHERE singleton")
assert sql("SELECT content.report_profile_ready()", role="board_migrator") == "t"
assert rows() == changed

# Independent staff history, shared sessions and another board stay byte-exact.
unchanged_sql = """SELECT jsonb_agg(jsonb_build_array(relation,value) ORDER BY relation,value)::text
FROM public.historical_rows() WHERE relation IN('staff_identity.accounts','content.moderation_audit','post_secrets.anonymous_sessions','media.jobs','media.assets','media_intake.handles')
OR (value->>'board'='hother') OR (relation='content.posts' AND value->>'id'='31001001')
OR (relation='content.threads' AND value->>'id'='31001001')"""
unrelated = sql(unchanged_sql)
batches = []
for _ in range(8):
    plan = preview()
    if not (plan["report_ids"] or plan["post_ids"] or plan["thread_ids"]):
        break
    result = apply(plan)
    assert result["applied"] and result["receipt"] == plan["receipt"]
    assert all(len(result[k]) <= 64 for k in ("report_ids","post_ids","thread_ids"))
    batches.append(result)
else:
    raise AssertionError("Owned finite fixture did not drain")
assert len(batches) >= 4, "Fixture did not exercise bounded continuation"
assert sum(len(p["report_ids"]) for p in batches) == 70
assert sum(len(p["post_ids"]) for p in batches) == 132
assert sum(len(p["thread_ids"]) for p in batches) == 1
assert sql(unchanged_sql) == unrelated, "Independent state changed"
assert sql("SELECT metadata_erased AND filename='[deleted]' AND bytes=1 AND width=1 AND height=1 AND file_deleted FROM content.post_media WHERE post_id=31000001") == "t"
assert sql("SELECT job_id=repeat('b1',16) AND asset_id=repeat('b4',16) FROM content.post_media WHERE post_id=31000001") == "t"
assert sql("SELECT count(*) FROM content.posts WHERE board='hclean' AND content_erased") == "132"
assert sql("SELECT count(*) FROM content.reports WHERE board='hclean'") == "1"
assert sql("SELECT count(*) FROM post_secrets.report_membership WHERE board='hclean'") == "1"
assert sql("SELECT count(*) FROM post_secrets.report_group WHERE board='hclean'") == "1"
for table in ("report_weight_evidence","anonymous_reports","report_post_snapshots"):
    assert sql(f"SELECT count(*) FROM post_secrets.{table} WHERE report_id BETWEEN 31010001 AND 31010070") == "0"
assert sql("SELECT used_bytes=(SELECT sum(charged_bytes) FROM post_secrets.report_post_snapshots) FROM post_secrets.report_snapshot_capacity") == "t"
assert sql("SELECT count(*) FROM post_secrets.deletion WHERE post_id BETWEEN 31000001 AND 31001002") == "1"
assert sql("SELECT count(*) FROM post_secrets.anonymous_posts WHERE post_id BETWEEN 31000001 AND 31001002") == "1"
assert sql("SELECT count(*) FROM post_secrets.op_peers WHERE thread_id=31000001") == "0"
assert sql("SELECT count(*) FROM post_secrets.op_replies WHERE thread_id=31000001") == "0"
# Repeated old receipts do not affect later batches or live data.
after = rows()
apply(first, ok=False)
assert rows() == after
sql("UPDATE content.posts SET deleted=false,comment='revive' WHERE id=31000001", role="board_migrator", ok=False)
sql("UPDATE content.threads SET deleted=false WHERE id=31000001", role="board_migrator", ok=False)
assert rows() == after
private = preview("hprivate")
assert private["post_ids"] == [31003001]
apply(private)
private = preview("hprivate")
assert private["thread_ids"] == [31003001]
apply(private)
assert preview("hprivate")["post_ids"] == []
print("Historical operator preview, bounded continuation, stale/role/catalog refusals, private boards, cascades and irreversible tombstones passed.")
