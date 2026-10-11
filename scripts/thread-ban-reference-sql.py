"""Compare actual thread batch rows with independently executed source traces."""
import hashlib
import html
import json
from datetime import datetime, timezone
from pathlib import Path


def literal(value):
    return "'" + str(value).replace("'", "''") + "'"


def main():
    fixture = json.loads(Path("crates/domain/tests/fixtures/thread-ban-source.json").read_text())
    print("SET TIME ZONE 'UTC'; SET ROLE board_migrator;")
    print("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES('tbref136','Owned source comparison','Synthetic',2000,1000,1000,1000,10);")
    print("UPDATE staff_identity.accounts SET allow_boards=ARRAY['all'],flags=ARRAY['threadban'] WHERE id=13400001; RESET ROLE;")
    for index, case in enumerate(fixture["cases"]):
        offset = 13610000 + index * 1000
        root = offset + 100
        def actor(host):
            return hashlib.sha256(f"thread-case-{index}:{host}".encode()).hexdigest()
        print("SET ROLE board_migrator;")
        print(f"INSERT INTO content.threads(id,board) VALUES({root},'tbref136');")
        for post in case["posts"]:
            no = offset + post["no"]
            print(f"INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES({no},'tbref136',{root},'Synthetic','Source comparison','Owned synthetic text');")
            print(f"INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at) VALUES({no},'tbref136',{root},decode('{actor(post['host'])}','hex'),extract(epoch FROM clock_timestamp())::bigint);")
        target = offset + case["no"] if case["no"] else 0
        op_days = case["observed"]["parsed_days"]["op"]
        rep_days = case["observed"]["parsed_days"]["rep"]
        call = ("content.issue_thread_restrictions(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),900,"
                f"'tbref136',{target},{literal(case['op_reason'])},{literal(case['rep_reason'])},"
                f"{op_days}::integer,{rep_days}::integer)")
        normal = bool(case["observed"]["deletions"]) and case["no"] == 100
        if not normal:
            # Runtime validates the PHP cast before narrowing to SQL's int32.
            # For wider source casts, qualify SQL's parameter conversion refusal
            # too; Rust/HTTP tests independently check the actual form boundary.
            if any(not -2147483648 <= days <= 2147483647 for days in (op_days, rep_days)):
                code = "22003"
            else:
                code = "P134N" if case["no"] in (999, 102) else "23514"
            print("RESET ROLE; SET ROLE board_staff;")
            print(f"DO $$ BEGIN PERFORM * FROM {call}; RAISE EXCEPTION 'Thread case {index} was accepted'; EXCEPTION WHEN SQLSTATE '{code}' THEN NULL; END $$;")
            print("RESET ROLE;")
            continue
        print("RESET ROLE; BEGIN; SET LOCAL ROLE board_staff;")
        print(f"DO $$ BEGIN PERFORM * FROM {call}; END $$;")
        print(f"UPDATE content.posts SET deleted=true WHERE board='tbref136' AND thread_id={root};")
        print(f"UPDATE content.threads SET deleted=true WHERE board='tbref136' AND id={root}; COMMIT;")
        expected = []
        for ban in case["observed"]["bans"]:
            seconds = int((datetime.strptime(str(ban["length"]), "%Y%m%d%H%M%S")
                           .replace(tzinfo=timezone.utc)).timestamp()) - fixture["request_at"]
            expected.append(dict(no=ban["post_num"], actor=actor(ban["host"]),
                                 **{"global": bool(ban["global"])}, warning=seconds == 0,
                                 seconds=seconds, reason=html.unescape(ban["reason"].split("<>")[0]),
                                 private=f"Thread Ban No.{root}", automatic=False, audits=1))
            assert "password" not in ban, "Source batch acquired posting equality"
        expected.sort(key=lambda row: row["no"])
        encoded = literal(json.dumps(expected, separators=(",", ":")))
        print("SET ROLE board_restriction_owner;")
        print(f"""DO $$ DECLARE actual jsonb; BEGIN
 SELECT jsonb_agg(jsonb_build_object('no',b.post_id-{offset},'actor',encode(b.actor_hash,'hex'),
  'global',b.global,'warning',b.warning,'seconds',extract(epoch FROM b.ends_at-b.starts_at)::bigint,
  'reason',b.public_reason,'private',b.private_reason,'automatic',b.automatic_identity IS NOT NULL,
  'audits',(SELECT count(*) FROM restrictions.audit a WHERE a.ban_id=b.id)) ORDER BY b.post_id)
 INTO actual FROM restrictions.bans b WHERE b.board='tbref136' AND b.post_id BETWEEN {offset} AND {offset+999};
 IF actual IS DISTINCT FROM {encoded}::jsonb THEN RAISE EXCEPTION 'Thread source case {index} differs'; END IF;
 IF EXISTS(SELECT 1 FROM restrictions.pending_thread_batches) THEN RAISE EXCEPTION 'Committed thread promise retained credentials'; END IF;
 END $$; RESET ROLE;""")
    print("DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Thread source comparison changed readiness'; END IF; END $$;")


if __name__ == "__main__":
    main()
