#!/usr/bin/env python3
"""Compare the private history arithmetic with the pinned PHP oracle."""
import json
from pathlib import Path
cases=json.loads(Path('fixtures/janitor-history-cases.json').read_text())
assert len(cases)==128
print("SET ROLE board_restriction_owner; DO $check$ DECLARE c jsonb; actual jsonb; BEGIN")
print("FOR c IN SELECT value FROM jsonb_array_elements($cases$"+json.dumps(cases)+"$cases$::jsonb) LOOP")
print("""
 actual:=restrictions.summarize_request_history(c->'rows',(c->>'now')::bigint);
 IF actual IS DISTINCT FROM c->'expected' THEN RAISE EXCEPTION 'History differs: % vs %',actual,c; END IF;
END LOOP;
RAISE NOTICE '128 request history source cases passed';
END $check$; RESET ROLE;
""")
