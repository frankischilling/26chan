#!/usr/bin/env python3
"""Extract only public board policy from an operator-supplied reference checkout."""
import argparse
import hashlib
import html
import json
import re
from pathlib import Path

POLICY_KEYS = set("CATEGORY TITLE META_DESCRIPTION MAX_COM_CHARS MAX_LINES CODE_TAGS SJIS_TAGS SPOILERS REQUIRE_SUBJECT OP_MARKUP FORCED_ANON DISP_ID SHOW_COUNTRY_FLAGS ENABLE_BOARD_FLAGS BOARD_FLAGS_TYPE TEXT_ONLY MAX_RES MAX_IMGRES PAGE_MAX DEF_PAGES LOG_MAX ENABLE_ARCHIVE ARCHIVE_MAX_AGE JSON_TAIL_SIZE PERMASAGE_HOURS ENABLE_CATALOG ENABLE_JSON JANITOR_BOARD UPLOAD_BOARD MAX_KB ENABLE_WEBM ENABLE_WEBM_AUDIO MAX_WEBM_FILESIZE MAX_WEBM_DURATION RENZOKU RENZOKU2 RENZOKU3 NO_TEXTONLY GIF_ONLY PASS_ONLY ROBOT9000".split())


def policy(path):
    values = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        match = re.match(r"^[ \t]*([A-Z0-9_]+)[ \t]*=[ \t]*(.*)$", line)
        if match and match[1] in POLICY_KEYS:
            values[match[1]] = match[2].strip()
    return values


def extract(root):
    slugs = (root / "boardlist.txt").read_text().split()
    if len(slugs) != len(set(slugs)) or any(not re.fullmatch(r"[a-z0-9]{1,10}", slug) for slug in slugs):
        raise ValueError("Board list has duplicate or invalid identifiers.")
    paths = [root / "boardlist.txt", root / "www.4chan/data/boards.php", root / "config/global_config.ini"]
    names = dict(re.findall(r'"dir"=>"([a-z0-9]+)","name"=>"([^"]+)"', paths[1].read_text()))
    boards = []
    for position, slug in enumerate(slugs + sorted({p.name.split(".")[0] for p in (root / "config/boards").glob("*.config.ini")} - set(slugs))):
        path = root / f"config/boards/{slug}.config.ini"
        overrides = policy(path)
        category = root / f"config/categories/{overrides['CATEGORY']}.config.ini"
        values = policy(paths[2]) | policy(category) | overrides
        paths.extend([category, path])
        boolean = lambda key: values.get(key, "no").lower() == "yes"
        integer = lambda key: int(values[key])
        title = names.get(slug)
        if title is None:
            match = re.search(rf"/{slug}/\s*-\s*([^\";]+)", html.unescape(values.get("META_DESCRIPTION", "")))
            title = match[1].strip() if match else values.get("TITLE", f"/{slug}/")
        if "TITLE" in overrides:
            title = re.sub(rf"^/{slug}/\s*-\s*", "", overrides["TITLE"])
        # These finite limits are the application's existing response budget.
        # /j/'s unbounded source history needs a separate staff-only interface.
        maximum = integer("PAGE_MAX") * integer("DEF_PAGES") if integer("PAGE_MAX") else min(integer("LOG_MAX"), 1000)
        board = {
            "slug": slug, "title": html.unescape(title),
            "description": html.unescape(values.get("META_DESCRIPTION", "")),
            "source_order": position, "listed": slug in slugs,
            "worksafe": values["CATEGORY"] == "ws",
            "max_comment_chars": integer("MAX_COM_CHARS"),
            "comment_max_lines": integer("MAX_LINES"),
            "comment_code_spacing": boolean("CODE_TAGS"),
            "comment_sjis_spacing": boolean("SJIS_TAGS"),
            "comment_spoiler_cleanup": boolean("SPOILERS"),
            "require_subject": boolean("REQUIRE_SUBJECT"),
            "op_markup": boolean("OP_MARKUP"), "forced_anon": boolean("FORCED_ANON"),
            "user_ids": boolean("DISP_ID"), "country_flags": boolean("SHOW_COUNTRY_FLAGS"),
            "text_only": boolean("TEXT_ONLY"), "reply_limit": 1000,
            "bump_limit": integer("MAX_RES"), "image_limit": integer("MAX_IMGRES"),
            "thread_limit": maximum, "threads_per_page": integer("DEF_PAGES"),
            "archive_retention_seconds": integer("ARCHIVE_MAX_AGE") * 3600 if boolean("ENABLE_ARCHIVE") else 0,
            "json_tail_size": integer("JSON_TAIL_SIZE"), "permasage_hours": integer("PERMASAGE_HOURS"),
            "catalog_enabled": boolean("ENABLE_CATALOG"), "json_enabled": boolean("ENABLE_JSON"),
            "staff_only": boolean("JANITOR_BOARD"), "upload_board": boolean("UPLOAD_BOARD"),
            "source_policy": values,
        }
        boards.append(board)
    hashes = {str(path.relative_to(root)).replace("\\", "/"): hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(set(paths))}
    return {"reference": "operator-supplied 4chan-old checkout", "listed_count": len(slugs), "files": hashes, "boards": boards}


def migration(reference):
    columns = [key for key in reference["boards"][0] if key not in {"listed", "source_policy"}]
    def sql(value):
        if isinstance(value, bool):
            return "true" if value else "false"
        if isinstance(value, int):
            return str(value)
        return "'" + value.replace("'", "''") + "'"
    rows = ["(" + ",".join(sql(board[key]) for key in columns) + ")" for board in reference["boards"]]
    return ("-- Public board definitions extracted from the pinned, operator-supplied checkout.\n"
            "-- Updates policy only; existing posts, identities and saved format profiles remain.\n"
            "INSERT INTO content.boards(" + ",".join(columns) + ") VALUES\n" + ",\n".join(rows)
            + "\nON CONFLICT(slug) DO UPDATE SET\n" + ",\n".join(key + "=EXCLUDED." + key for key in columns if key != "slug") + ";\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--migration", type=Path)
    args = parser.parse_args()
    reference = extract(args.source)
    data = json.dumps(reference, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if args.output.read_text(encoding="utf-8") != data:
            raise SystemExit("Board reference differs from the supplied checkout.")
        if args.migration and args.migration.read_text(encoding="utf-8") != migration(reference):
            raise SystemExit("Board migration differs from the extracted policy.")
        print("Board reference matches all listed and additional configuration files.")
    else:
        args.output.write_text(data, encoding="utf-8")
        if args.migration:
            args.migration.write_text(migration(reference), encoding="utf-8")


if __name__ == "__main__":
    main()
