"""Compare private SQL report policy to independently executed PHP results."""
import json
from pathlib import Path

fixture = json.loads((Path(__file__).resolve().parents[1] /
                      "crates/domain/tests/fixtures/report-ban-enforcement.json").read_text())
assert len(fixture["cases"]) == 3456
print("CREATE TEMP TABLE source_report_ban_cases(payload jsonb NOT NULL);")
print("COPY source_report_ban_cases FROM STDIN;")
for case in fixture["cases"]:
    print(json.dumps(case, separators=(",", ":")).replace("\\", "\\\\"))
print(r"\.")
print("GRANT SELECT ON source_report_ban_cases TO board_restriction_owner;")
print("SET ROLE board_restriction_owner;")
print("""
DO $$ DECLARE c jsonb;
BEGIN
 FOR c IN SELECT payload FROM source_report_ban_cases LOOP
  IF restrictions.source_report_decision(c->'bans','test',(c->>'check_ban')::boolean,
    (c->>'pass_present')::boolean,(c->>'bypass')::boolean,(c->>'password_present')::boolean)
    IS DISTINCT FROM (c->>'blocked')::boolean THEN
   RAISE EXCEPTION 'SQL report restriction differs from executed source in %',c->>'name';
  END IF;
 END LOOP;
END $$;
RESET ROLE;
""")
