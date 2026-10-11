#!/usr/bin/env python3
"""Emit only synthetic oracle cases as a PostgreSQL COPY stream."""
import json
from pathlib import Path


def main():
    root = Path(__file__).resolve().parents[1]
    fixture = json.loads(
        (root / "crates/domain/tests/fixtures/ban-enforcement.json").read_text()
    )
    cases = fixture["cases"]
    if len(cases) != 13840:
        raise ValueError("Unexpected source comparison case count")
    print("CREATE TEMP TABLE source_restriction_cases (payload jsonb NOT NULL);")
    print("COPY source_restriction_cases FROM STDIN;")
    for case in cases:
        # COPY text escaping is separate from JSON escaping.
        print(json.dumps(case, separators=(",", ":")).replace("\\", "\\\\"))
    print(r"\.")
    print("GRANT SELECT ON source_restriction_cases TO board_restriction_owner;")
    print("SET ROLE board_restriction_owner;")
    print("""
DO $$ DECLARE c jsonb; v_rows jsonb; v_disposition text; v_expired bigint[];
BEGIN
 FOR c IN SELECT payload FROM source_restriction_cases LOOP
  SELECT coalesce(jsonb_agg(b.row ORDER BY f.position,b.position),'[]'::jsonb) INTO v_rows
  FROM jsonb_array_elements(c->'field_order'||'["host"]'::jsonb) WITH ORDINALITY f(key,position)
  CROSS JOIN jsonb_array_elements(c->'bans') WITH ORDINALITY b(row,position)
  WHERE b.row->>'key'=f.key#>>'{}' AND (b.row->>'active')::boolean
    AND (f.key#>>'{}'<>'4pass_id' OR (c->>'pass_present')::boolean);
  SELECT disposition,expired INTO v_disposition,v_expired
   FROM restrictions.source_decision(v_rows,'test',1700000000,
     (c->>'pass_present')::boolean,(c->>'bypass')::boolean,(c->>'verified')::boolean);
  IF v_disposition IS DISTINCT FROM (CASE (c->>'decision')::integer
      WHEN 0 THEN 'allowed' WHEN 1 THEN 'banned' WHEN 2 THEN 'warned' END)
    OR to_jsonb(v_expired) IS DISTINCT FROM c->'expired' THEN
   RAISE EXCEPTION 'SQL differs from executed source in %',c->>'name';
  END IF;
 END LOOP;
END $$;
RESET ROLE;
""")


if __name__ == "__main__":
    main()
