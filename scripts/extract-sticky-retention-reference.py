#!/usr/bin/env python3
"""Pin the old sticky/undead reply pruning query with isolated SQL witnesses.

The source checkout is read only. SQLite executes the reference's two SELECT
predicates on synthetic rows. This is not a boot of the original PHP server.
"""

import argparse
import hashlib
import json
import pathlib
import re
import sqlite3


SOURCE_HASH = "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445"
GLOBAL_HASH = "8bebeedec119b30559cba4fdccfef416653294cf62118d10288434be99a3034d"


def checked_source(root):
    source = root / "imgboard.php"
    config = root / "config/global_config.ini"
    for path, expected in ((source, SOURCE_HASH), (config, GLOBAL_HASH)):
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError(f"Source hash mismatch: {path}")
    content = source.read_text(encoding="utf-8")
    start = content.index("// Remove old replies if the thread is sticky+undead")
    end = content.index("// April 2024", start)
    block = content[start:end]
    for anchor in (
        "$is_undead_sticky && STICKY_CAP > 1",
        "SELECT MIN(no) FROM (SELECT no FROM",
        "ORDER BY no DESC LIMIT ",
        "(STICKY_CAP - 1)",
        "WHERE resto = $resto AND no < $min_no",
        "delete_post((int)$prune_row['no'], '', 0, 1, 1, 0)",
    ):
        if anchor not in block:
            raise ValueError(f"Source pruning block changed: {anchor}")
    match = re.search(r"^STICKY_CAP\s*=\s*(\d+)\s*$", config.read_text(), re.M)
    if not match or int(match[1]) != 1000:
        raise ValueError("Pinned STICKY_CAP must equal 1000")
    return int(match[1]), hashlib.sha256(block.encode("utf-8")).hexdigest()


def witness(name, cap, existing, incoming):
    con = sqlite3.connect(":memory:")
    con.execute("CREATE TABLE posts (no INTEGER PRIMARY KEY, resto INTEGER NOT NULL)")
    con.executemany("INSERT INTO posts VALUES (?, 1)", ((no,) for no in existing))
    # Other threads and the OP can never enter the reply window.
    con.executemany("INSERT INTO posts VALUES (?, 99999)", ((no,) for no in (8, 9)))
    con.execute("INSERT INTO posts VALUES (1, 0)")
    pruned = []
    if cap > 1:
        boundary = con.execute(
            "SELECT MIN(no) FROM (SELECT no FROM posts WHERE resto = 1 ORDER BY no DESC LIMIT ?)",
            (cap - 1,),
        ).fetchone()[0]
        if boundary is not None and boundary > 1:
            pruned = [row[0] for row in con.execute(
                "SELECT no FROM posts WHERE resto = 1 AND no < ? ORDER BY no", (boundary,)
            )]
            con.executemany("DELETE FROM posts WHERE no=?", ((no,) for no in pruned))
    con.execute("INSERT INTO posts VALUES (?, 1)", (incoming,))
    remaining = [row[0] for row in con.execute(
        "SELECT no FROM posts WHERE resto=1 ORDER BY no"
    )]
    assert [row[0] for row in con.execute("SELECT no FROM posts WHERE resto=99999 ORDER BY no")] == [8, 9]
    con.close()
    return {
        "case": name,
        "capacity": cap,
        "existing": existing,
        "incoming": incoming,
        "pruned": pruned,
        "surviving": remaining,
        "visible_reply_count": len(remaining),
    }


def generate(root):
    limit, block_hash = checked_source(root)
    rows = [
        witness("disabled-cap-one", 1, [10, 20], 30),
        witness("empty", 3, [], 30),
        witness("below-cap", 3, [10], 30),
        witness("last-slot", 3, [10, 20], 30),
        witness("one-oldest", 3, [10, 20, 30], 40),
        witness("multiple-oldest", 3, [10, 20, 30, 40, 50], 60),
        witness("sparse-ids", 3, [100, 190, 250, 900], 1000),
        witness("pinned-first-overflow", limit, list(range(10, 10 + limit)), 2000),
        witness("pinned-existing-excess", limit, list(range(10, 10 + limit + 6)), 2000),
    ]
    return {
        "reference": "operator-supplied imgboard.php sticky+undead block",
        "source_sha256": SOURCE_HASH,
        "config_sha256": GLOBAL_HASH,
        "pruning_block_sha256": block_hash,
        "source_sticky_cap": limit,
        "note": "Independent SQLite evaluation of the old PHP's exact two SELECT predicates; the source runtime was not booted",
        "cases": rows,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=pathlib.Path, required=True)
    parser.add_argument("--fixture", type=pathlib.Path, default=pathlib.Path("fixtures/sticky-retention-reference.json"))
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    expected = json.dumps(generate(args.source), indent=2, ensure_ascii=False) + "\n"
    if args.write:
        args.fixture.parent.mkdir(parents=True, exist_ok=True)
        args.fixture.write_text(expected, encoding="utf-8")
    elif args.fixture.read_text(encoding="utf-8") != expected:
        raise SystemExit("Reference fixture differs from pinned source")


if __name__ == "__main__":
    main()
