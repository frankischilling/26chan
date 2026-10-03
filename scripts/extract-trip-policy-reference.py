#!/usr/bin/env python3
"""Extract the pinned public trip-suppression setting without changing old imports."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def extract(source):
    project = Path(__file__).resolve().parents[1]
    pinned = json.loads((project / "fixtures/board-reference.json").read_text(encoding="utf-8"))
    for relative, expected in pinned["files"].items():
        path = source / relative
        if path.stat().st_size > 131072 or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError("Pinned public board configuration differs.")
    spec = importlib.util.spec_from_file_location("owned_public_boards", project / "scripts/extract-board-reference.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.POLICY_KEYS.add("STRIP_TRIPCODE")
    board_source = module.extract(source)
    if board_source["files"] != pinned["files"] or board_source["listed_count"] != 80:
        raise ValueError("Pinned board inventory differs.")
    boards = []
    for board in board_source["boards"]:
        value = board["source_policy"].get("STRIP_TRIPCODE")
        if value not in {"yes", "no"}:
            raise ValueError("Source trip suppression has no audited boolean value.")
        boards.append({"slug": board["slug"], "strip_tripcode": value == "yes"})
    if len(boards) != 82:
        raise ValueError("Trip policy inventory is incomplete.")
    return {"reference": "operator-supplied 4chan-old checkout", "files": pinned["files"],
            "listed_count": board_source["listed_count"], "boards": boards}


def migration(reference):
    rows = ["('" + board["slug"] + "'," + ("true" if board["strip_tripcode"] else "false") + ")"
            for board in reference["boards"]]
    return ("-- New posts follow the pinned STRIP_TRIPCODE setting; saved identities remain.\n"
            "ALTER TABLE content.boards ADD COLUMN strip_tripcode boolean NOT NULL DEFAULT false;\n"
            "GRANT SELECT(strip_tripcode) ON content.boards TO board_attachment_owner;\n"
            "UPDATE content.boards b SET strip_tripcode=policy.enabled FROM (VALUES\n"
            + ",\n".join(rows) + ") policy(slug,enabled) WHERE b.slug=policy.slug;\n\n"
            "CREATE OR REPLACE FUNCTION content.apply_post_trip() RETURNS trigger\n"
            "LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$\n"
            "DECLARE v_suppressed boolean;\n"
            "BEGIN\n"
            "    SELECT forced_anon OR strip_tripcode INTO v_suppressed\n"
            "    FROM content.boards WHERE slug=NEW.board FOR SHARE;\n"
            "    IF NOT FOUND THEN\n"
            "        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';\n"
            "    END IF;\n"
            "    IF v_suppressed AND NEW.name='' THEN\n"
            "        NEW.name := 'Anonymous';\n"
            "    END IF;\n"
            "    NEW.trip := CASE WHEN v_suppressed THEN NULL\n"
            "        ELSE nullif(current_setting('board.post_trip', true), '') END;\n"
            "    RETURN NEW;\n"
            "END $$;\n"
            "REVOKE ALL ON FUNCTION content.apply_post_trip() FROM PUBLIC;\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--migration", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    reference = extract(args.source)
    outputs = {args.output: json.dumps(reference, ensure_ascii=False, indent=2) + "\n"}
    if args.migration:
        outputs[args.migration] = migration(reference)
    for path, value in outputs.items():
        encoded = value.encode("utf-8")
        if args.check:
            if path.read_bytes() != encoded:
                raise ValueError("Trip suppression reference or migration differs.")
        else:
            path.write_bytes(encoded)
    print(f"Trip policy: {len(reference['boards'])} definitions, "
          f"{sum(board['strip_tripcode'] for board in reference['boards'])} suppress trips.")


if __name__ == "__main__":
    main()
