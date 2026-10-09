#!/usr/bin/env python3
"""Extract only public board policy from an operator-supplied reference checkout."""
import argparse
import hashlib
import html
import json
import re
from pathlib import Path

POLICY_KEYS = set("CATEGORY TITLE META_DESCRIPTION MAX_COM_CHARS MAX_LINES CODE_TAGS SJIS_TAGS SPOILERS REQUIRE_SUBJECT OP_MARKUP FORCED_ANON DISP_ID SHOW_COUNTRY_FLAGS ENABLE_BOARD_FLAGS BOARD_FLAGS_TYPE TEXT_ONLY MAX_RES MAX_IMGRES PAGE_MAX DEF_PAGES LOG_MAX ENABLE_ARCHIVE ARCHIVE_MAX_AGE JSON_TAIL_SIZE PERMASAGE_HOURS ENABLE_CATALOG ENABLE_JSON JANITOR_BOARD UPLOAD_BOARD USE_RSS MAX_KB ENABLE_WEBM ENABLE_WEBM_AUDIO MAX_WEBM_FILESIZE MAX_WEBM_DURATION RENZOKU RENZOKU2 RENZOKU3 NO_TEXTONLY GIF_ONLY PASS_ONLY ROBOT9000 WORD_FILT".split())


POLICY_KEYS.add("REPLIES_SHOWN")
POLICY_KEYS.add("EXPIRE_NEGLECTED")
POLICY_KEYS.add("META_BOARD")
POLICY_KEYS.add("DISP_ID_NO_HEAVEN")
POLICY_KEYS.update({"MAX_USER_THREADS", "MAX_USER_THREADS_PERIOD"})
POLICY_KEYS.add("CAN_REPORT_POSTS")
POLICY_KEYS.add("JSMATH")
POLICY_KEYS.add("SHOW_BLOTTER")
POLICY_KEYS.add("SUBTITLE")
POLICY_KEYS.update({"ENABLE_PAINTERJS", "ENABLE_OEKAKI_REPLAYS", "PAINTERJS_DIMS"})


def policy(path):
    values = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        match = re.match(r"^[ \t]*([A-Z0-9_]+)[ \t]*=[ \t]*(.*)$", line)
        if match and match[1] in POLICY_KEYS:
            values[match[1]] = match[2].strip()
    return values


FICTION_SUBTITLE = "The stories and information posted here are artistic works of fiction and falsehood.<br>Only a fool would take anything posted here as fact."
WORKSAFE_SUBTITLE = 'Worksafe Board: /<a href="//boards.4chan.org/wsg/" title="Worksafe GIF">wsg</a>/'


def subtitle_profile(values):
    subtitle = values.get("SUBTITLE")
    profiles = {None: "none", FICTION_SUBTITLE: "fiction", WORKSAFE_SUBTITLE: "worksafe_gif"}
    if subtitle not in profiles:
        raise ValueError("Unaudited source SUBTITLE; add a reviewed fixed profile before import.")
    return profiles[subtitle]


def subtitle_migration(reference):
    rows = []
    for board in reference["boards"]:
        if board["board_subtitle"] != "none":
            rows.append("('" + board["slug"] + "','" + board["board_subtitle"] + "')")
    return ("-- Audited source SUBTITLE profiles; descriptions and existing policy stay unchanged.\n"
            "-- Only fixed template markup may render these profiles.\n"
            "ALTER TABLE content.boards ADD COLUMN board_subtitle text NOT NULL DEFAULT 'none'\n"
            "    CONSTRAINT boards_subtitle_profile CHECK (board_subtitle IN ('none','fiction','worksafe_gif'));\n"
            "UPDATE content.boards b SET board_subtitle=policy.profile FROM (VALUES\n" +
            ",\n".join(rows) + ") policy(slug,profile) WHERE b.slug=policy.slug;\n")


def short_title(slug, title, historical_titles=False):
    prefix = rf"/{slug}/"
    if slug == "s4s" and not historical_titles:
        prefix = rf"(?:{prefix}|\[s4s\])"
    return re.sub(rf"^{prefix}\s*-\s*", "", title)


def extract(root, names_encoding="utf-8", historical_titles=False):
    slugs = (root / "boardlist.txt").read_text(encoding="utf-8").split()
    if len(slugs) != len(set(slugs)) or any(not re.fullmatch(r"[a-z0-9]{1,10}", slug) for slug in slugs):
        raise ValueError("Board list has duplicate or invalid identifiers.")
    paths = [root / "boardlist.txt", root / "www.4chan/data/boards.php", root / "config/global_config.ini"]
    names = dict(re.findall(r'"dir"=>"([a-z0-9]+)","name"=>"([^"]+)"', paths[1].read_text(encoding=names_encoding)))
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
            title = short_title(slug, overrides["TITLE"], historical_titles)
        # These finite limits are the application's existing response budget.
        # /j/'s unbounded source history needs a separate staff-only interface.
        maximum = integer("PAGE_MAX") * integer("DEF_PAGES") if integer("PAGE_MAX") else min(integer("LOG_MAX"), 1000)
        board = {
            "slug": slug, "title": html.unescape(title),
            "description": html.unescape(values.get("META_DESCRIPTION", "")),
            "source_order": position, "listed": slug in slugs,
            "worksafe": values["CATEGORY"] == "ws",
            "show_blotter": boolean("SHOW_BLOTTER"),
            "board_subtitle": subtitle_profile(values),
            "max_comment_chars": integer("MAX_COM_CHARS"),
            "comment_max_lines": integer("MAX_LINES"),
            "comment_code_spacing": boolean("CODE_TAGS"),
            "comment_sjis_spacing": boolean("SJIS_TAGS"),
            "math_tags": boolean("JSMATH"),
            "oekaki": boolean("ENABLE_PAINTERJS"),
            "oekaki_replays": boolean("ENABLE_OEKAKI_REPLAYS"),
            "oekaki_width": integer("PAINTERJS_DIMS"),
            "oekaki_height": integer("PAINTERJS_DIMS"),
            "comment_spoiler_cleanup": boolean("SPOILERS"),
            "require_subject": boolean("REQUIRE_SUBJECT"),
            "op_markup": boolean("OP_MARKUP"), "forced_anon": boolean("FORCED_ANON"),
            "user_ids": boolean("DISP_ID"), "country_flags": boolean("SHOW_COUNTRY_FLAGS"),
            "text_only": boolean("TEXT_ONLY"), "reply_limit": 1000,
            "bump_limit": integer("MAX_RES"), "image_limit": integer("MAX_IMGRES"),
            "thread_limit": maximum, "threads_per_page": integer("DEF_PAGES"),
            "replies_shown": integer("REPLIES_SHOWN"),
            "expire_neglected": boolean("EXPIRE_NEGLECTED"),
            "posting_reply_seconds": integer("RENZOKU"),
            "posting_image_seconds": integer("RENZOKU2"),
            "posting_thread_seconds": integer("RENZOKU3"),
            "user_thread_limit": integer("MAX_USER_THREADS"),
            "user_thread_period_hours": integer("MAX_USER_THREADS_PERIOD"),
            "can_report_posts": boolean("CAN_REPORT_POSTS"),
            "archive_retention_seconds": integer("ARCHIVE_MAX_AGE") * 3600 if boolean("ENABLE_ARCHIVE") else 0,
            "json_tail_size": integer("JSON_TAIL_SIZE"), "permasage_hours": integer("PERMASAGE_HOURS"),
            "catalog_enabled": boolean("ENABLE_CATALOG"), "json_enabled": boolean("ENABLE_JSON"),
            "staff_only": boolean("JANITOR_BOARD"), "upload_board": boolean("UPLOAD_BOARD"),
            "meta_board": boolean("META_BOARD"),
            "poster_id_no_heaven": boolean("DISP_ID_NO_HEAVEN"),
            "source_policy": values,
        }
        boards.append(board)
    hashes = {str(path.relative_to(root)).replace("\\", "/"): hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(set(paths))}
    return {"reference": "operator-supplied 4chan-old checkout", "listed_count": len(slugs), "files": hashes, "boards": boards}


def migration(reference):
    # Later policy columns must not rewrite the already-applied 0045 import.
    columns = [key for key in reference["boards"][0] if key not in {
        "listed", "source_policy", "meta_board", "poster_id_no_heaven", "expire_neglected",
        "posting_reply_seconds", "posting_image_seconds", "posting_thread_seconds",
        "user_thread_limit", "user_thread_period_hours", "can_report_posts", "math_tags",
        "oekaki", "oekaki_replays", "oekaki_width", "oekaki_height", "show_blotter", "board_subtitle", "replies_shown",
    }]
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


def rss_migration(reference):
    rows = ["('" + board["slug"] + "'," +
            ("true" if board["source_policy"]["USE_RSS"].lower() == "yes" else "false") + ")"
            for board in reference["boards"]]
    return ("-- Feed policy extracted from the same pinned board configuration.\n"
            "ALTER TABLE content.boards ADD COLUMN rss_enabled boolean NOT NULL DEFAULT true;\n"
            "UPDATE content.boards b SET rss_enabled=policy.enabled FROM (VALUES\n" +
            ",\n".join(rows) + ") policy(slug,enabled) WHERE b.slug=policy.slug;\n")


def board_title_updates(reference, historical):
    rows = []
    for board, original in zip(reference["boards"], historical["boards"], strict=True):
        if board["slug"] != original["slug"]:
            raise ValueError("Historical board order differs.")
        if board["title"] != original["title"]:
            quote = lambda value: "'" + value.replace("'", "''") + "'"
            rows.append("UPDATE content.boards SET title=" + quote(board["title"]) +
                        " WHERE slug=" + quote(board["slug"]) +
                        " AND title=" + quote(original["title"]) + ";\n")
    return "".join(rows)


def board_encoding_migration(reference, historical):
    return ("-- Correct source board titles decoded with the historical Windows default.\n"
            "-- Preserve the applied migration checksum and operator-edited titles.\n" +
            board_title_updates(reference, historical))


def board_title_migration(reference, historical):
    return ("-- Store the short name from the source [s4s] TITLE override.\n"
            "-- Preserve the applied import checksum and operator-edited titles.\n" +
            board_title_updates(reference, historical))


def wordfilter_policy(reference, source):
    profiles = {"global": 0, "ck": 1, "int": 1, "asp": 2, "v": 3, "test": 4, "vg": 0, "vp": 0}
    rows = []
    for board in reference["boards"]:
        local = board["slug"] if (source / "wordfilters" / (board["slug"] + ".php")).is_file() else "global"
        if local not in profiles:
            raise SystemExit("An unaudited board wordfilter needs a fixed profile: " + local)
        enabled = board["source_policy"]["WORD_FILT"].lower()
        if enabled not in {"yes", "no"}:
            raise SystemExit("Unknown source WORD_FILT switch.")
        rows.append("('" + board["slug"] + "'," + ("true" if enabled == "yes" else "false") + "," + str(profiles[local]) + ")")
    return ("-- Pinned source WORD_FILT switches and board-file replacement of the global filter.\n"
            "UPDATE content.boards b SET word_filter_enabled=policy.enabled,word_filter_profile=policy.profile\n"
            "FROM (VALUES\n" + ",\n".join(rows) + ") policy(slug,enabled,profile) WHERE b.slug=policy.slug;\n")


def math_migration(reference):
    updates = []
    for board in reference["boards"]:
        if board["math_tags"]:
            slug = board["slug"].replace("'", "''")
            updates.append("UPDATE content.boards SET math_tags=true WHERE slug='" + slug + "';\n")
    return ("-- Pinned JSMATH display policy: disabled globally, enabled only on /sci/.\n"
            "-- Math is a browser projection; stored comments and API delimiters stay literal.\n"
            "ALTER TABLE content.boards ADD COLUMN math_tags boolean NOT NULL DEFAULT false;\n"
            + "".join(updates))


def drawing_migration(reference):
    rows = []
    for board in reference["boards"]:
        if (board["oekaki"], board["oekaki_replays"], board["oekaki_width"], board["oekaki_height"]) != (False, False, 400, 400):
            slug = board["slug"].replace("'", "''")
            rows.append("('" + slug + "'," +
                        ("true" if board["oekaki"] else "false") + "," +
                        ("true" if board["oekaki_replays"] else "false") + "," +
                        str(board["oekaki_width"]) + "," + str(board["oekaki_height"]) + ")")
    return ("-- Imported ENABLE_PAINTERJS, ENABLE_OEKAKI_REPLAYS and PAINTERJS_DIMS policy.\n"
            "-- Replay-enabled boards retain their policy while replay support is unfinished.\n"
            "-- The unused source OEKAKI_MIN/MAX constants are not admission limits.\n"
            "ALTER TABLE content.boards\n"
            "    ADD COLUMN oekaki boolean NOT NULL DEFAULT false,\n"
            "    ADD COLUMN oekaki_replays boolean NOT NULL DEFAULT false,\n"
            "    ADD COLUMN oekaki_width integer NOT NULL DEFAULT 400 CHECK (oekaki_width > 0),\n"
            "    ADD COLUMN oekaki_height integer NOT NULL DEFAULT 400 CHECK (oekaki_height > 0);\n"
            "UPDATE content.boards b SET oekaki=policy.enabled,oekaki_replays=policy.replays,\n"
            "    oekaki_width=policy.width,oekaki_height=policy.height\n"
            "FROM (VALUES\n" + ",\n".join(rows) +
            ") policy(slug,enabled,replays,width,height) WHERE b.slug=policy.slug;\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--migration", type=Path, help="Historical board import using the original name decoding and title extraction")
    parser.add_argument("--rss-migration", type=Path)
    parser.add_argument("--math-migration", type=Path)
    parser.add_argument("--drawing-migration", type=Path)
    parser.add_argument("--subtitle-migration", type=Path)
    parser.add_argument("--wordfilter-migration", type=Path)
    parser.add_argument("--board-encoding-migration", type=Path)
    parser.add_argument("--board-title-migration", type=Path)
    args = parser.parse_args()
    reference = extract(args.source)
    # Migrations 0045 and 0064 predate the square-bracket title correction.
    # Reproduce each applied migration with its original extraction behavior.
    historical = extract(args.source, names_encoding="cp1252", historical_titles=True)
    historical_utf8 = extract(args.source, historical_titles=True)
    data = json.dumps(reference, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if args.subtitle_migration and args.subtitle_migration.read_bytes() != subtitle_migration(reference).encode("utf-8"):
            raise SystemExit("Board subtitle migration differs from the extracted policy.")
        if args.output.read_text(encoding="utf-8") != data:
            raise SystemExit("Board reference differs from the supplied checkout.")
        if args.migration and args.migration.read_bytes() != migration(historical).encode("utf-8"):
            raise SystemExit("Board migration differs from the extracted policy.")
        if args.drawing_migration and args.drawing_migration.read_bytes() != drawing_migration(reference).encode("utf-8"):
            raise SystemExit("Drawing migration differs from the extracted policy.")
        if args.math_migration and args.math_migration.read_bytes() != math_migration(reference).encode("utf-8"):
            raise SystemExit("Math migration differs from the extracted policy.")
        if args.rss_migration and args.rss_migration.read_bytes() != rss_migration(reference).encode("utf-8"):
            raise SystemExit("RSS migration differs from the extracted policy.")
        if args.wordfilter_migration:
            marker = "-- Pinned source WORD_FILT switches and board-file replacement of the global filter.\n"
            actual = args.wordfilter_migration.read_text(encoding="utf-8")
            if marker not in actual or actual[actual.index(marker):] != wordfilter_policy(reference, args.source):
                raise SystemExit("Wordfilter migration differs from the extracted policy.")
        if args.board_encoding_migration and args.board_encoding_migration.read_bytes() != board_encoding_migration(historical_utf8, historical).encode("utf-8"):
            raise SystemExit("Board encoding migration differs from the extracted policy.")
        if args.board_title_migration and args.board_title_migration.read_bytes() != board_title_migration(reference, historical_utf8).encode("utf-8"):
            raise SystemExit("Board title migration differs from the extracted policy.")
        print("Board reference matches all listed and additional configuration files.")
    else:
        args.output.write_bytes(data.encode("utf-8"))
        if args.subtitle_migration:
            args.subtitle_migration.write_bytes(subtitle_migration(reference).encode("utf-8"))
        if args.migration:
            args.migration.write_bytes(migration(historical).encode("utf-8"))
        if args.drawing_migration:
            args.drawing_migration.write_bytes(drawing_migration(reference).encode("utf-8"))
        if args.math_migration:
            args.math_migration.write_bytes(math_migration(reference).encode("utf-8"))
        if args.rss_migration:
            args.rss_migration.write_bytes(rss_migration(reference).encode("utf-8"))
        if args.board_encoding_migration:
            args.board_encoding_migration.write_bytes(board_encoding_migration(historical_utf8, historical).encode("utf-8"))
        if args.board_title_migration:
            args.board_title_migration.write_bytes(board_title_migration(reference, historical_utf8).encode("utf-8"))


if __name__ == "__main__":
    main()
