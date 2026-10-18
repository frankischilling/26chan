#!/usr/bin/env python3
"""Qualify the SQL image-rule decision against isolated source outputs."""
import json
from pathlib import Path
root = Path(__file__).resolve().parent.parent
cases = json.loads((root / "crates/domain/tests/fixtures/janitor-file-effects.json").read_text())["cases"]
if len(cases) != 1728:
    raise SystemExit("Unexpected file-effect case count")
encoded = json.dumps(cases, separators=(",", ":"))
if "$cases$" in encoded:
    raise SystemExit("Unexpected fixture delimiter")
print("SET ROLE board_restriction_owner; DO $check$ DECLARE c jsonb; i jsonb; e jsonb; actual smallint; wanted smallint; raw boolean; n integer:=0; BEGIN")
print("FOR c IN SELECT value FROM jsonb_array_elements($cases$" + encoded + "$cases$::jsonb) LOOP")
print("""
 i:=c->'input'; e:=c->'expected'; n:=n+1;
 raw:=i->>'capture_policy'<>'everything' OR (i->>'illegal')::boolean
  OR ((i->>'amended')::boolean AND (i->>'file')::boolean AND i->>'review_policy'='json_only');
 actual:=restrictions.request_file_rule_decision((i->>'warning_action')::boolean,raw,
  (i->>'digest_present')::boolean,i->>'blacklist');
 wanted:=CASE WHEN (e->>'blacklist_count')::integer=0 THEN NULL
  WHEN (e->>'blacklist_ban')::boolean THEN 1 ELSE 0 END;
 IF actual IS DISTINCT FROM wanted THEN RAISE EXCEPTION 'File rule differs from source case %',n; END IF;
END LOOP;
BEGIN PERFORM restrictions.request_file_rule_decision(NULL,true,true,'image');
 RAISE EXCEPTION 'Unknown warning evidence accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
BEGIN PERFORM restrictions.request_file_rule_decision(false,true,true,'other');
 RAISE EXCEPTION 'Unknown file policy accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
RAISE NOTICE 'SQL file effects match % source cases',n;
END $check$; RESET ROLE;
""")
