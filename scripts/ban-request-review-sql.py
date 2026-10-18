#!/usr/bin/env python3
"""Compare the private SQL policy with executed, hash-pinned PHP decisions."""
import json
from pathlib import Path
root = Path(__file__).resolve().parent.parent
cases = json.loads((root / 'crates/domain/tests/fixtures/ban-request-review.json').read_text())
if len(cases) != 2016:
    raise SystemExit('Unexpected source review case count')
encoded = json.dumps(cases, separators=(',', ':'))
if '$cases$' in encoded:
    raise SystemExit('Unexpected source fixture delimiter')
print("SET ROLE board_restriction_owner;\nDO $check$ DECLARE c jsonb; i jsonb; e jsonb; d record; n integer:=0; BEGIN")
print("FOR c IN SELECT value FROM jsonb_array_elements($cases$" + encoded + "$cases$::jsonb) LOOP")
print("""
 i:=c->'input'; e:=c->'expected'; n:=n+1;
 BEGIN
  SELECT * INTO d FROM restrictions.request_review_decision(
   CASE i->>'template_duration' WHEN 'warning' THEN 0 WHEN 'finite' THEN 604800 ELSE NULL END,
   i->>'template_duration'='warning',(i->>'template_global')::boolean,(i->>'json_only')::boolean,
   (i->>'requested_warning')::boolean,(i->>'amended')::boolean,(i->>'days')::integer,
   (i->>'global_override')::boolean,(i->>'active_local')::boolean);
  IF e ? 'error' THEN RAISE EXCEPTION 'Invalid source review case % was accepted',n; END IF;
  IF jsonb_build_object('seconds',d.seconds,'warning_action',d.warning_action,'global',d.global,'duplicates',d.duplicates)
   IS DISTINCT FROM jsonb_build_object('seconds',e->'seconds','warning_action',e->'warning_action','global',e->'global','duplicates',e->'duplicates') THEN
   RAISE EXCEPTION 'SQL review differs from source case %',n; END IF;
 EXCEPTION WHEN check_violation THEN
  IF NOT e ? 'error' THEN RAISE EXCEPTION 'Valid source review case % was rejected',n; END IF;
 END;
END LOOP;
RAISE NOTICE 'SQL request review matches % isolated source cases',n;
END $check$;
RESET ROLE;
""")
