#!/usr/bin/env python3
"""Pin the source image duplicate gate and its effective board switches."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("source", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
hashes = {
    "admin.php": "4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb",
    "yotsuba_config.php": "c029e1d016e9b6e575c796784d0c7ccb3b89f80bceac0983d4468fcdf1a1e562",
    "imgboard.php": "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445",
    "config/global_config.ini": "8bebeedec119b30559cba4fdccfef416653294cf62118d10288434be99a3034d",
    "config/global_strings.ini": "3b3b60083db74fb1693af6f0824cccd6ec3ca446242474008cf9be474c65cd17",
    "config/boards/f.config.ini": "d9860308ce998306e08cb39a98420818f0a078da200e6a1e617672d0e169fa3a",
    "config/boards/test.config.ini": "72f1ddef8a63944bfb40e5d2e368c8ffeb42bef4c18b02a04d3638186a98b683",
    "config/boards/wsg.config.ini": "b8d30a18b219eaaa6f2ac1d1ae0a04ad2fe70ddaf39b63a3d8b3bb2e89e796e9",
}
texts = {}
for name, digest in hashes.items():
    raw = (args.source / name).read_bytes()
    assert hashlib.sha256(raw).hexdigest() == digest, name
    texts[name] = raw.decode().replace("\r\n", "\n")
source = texts["imgboard.php"]
block = source.split("\t\t// Infrequent flood check (dupe image)\n", 1)[1].split("\n\t\t$rootpredicate", 1)[0]
record = source.split("\t\tif (UPLOAD_BOARD && $md5 ) {", 1)[1].split("\n\t\t}", 1)[0]
settings = {
    name: [line for line in text.splitlines() if line.startswith(("MAX_IMG_REPOST_COUNT =", "SQLLOGMD5 =", "UPLOAD_BOARD =", "S_DUPE ="))]
    for name, text in texts.items() if name.startswith("config/")
}
result = {
    "source": hashes,
    "gate_php": "function source_image_duplicate($has_image, $capcode, $resto, $md5, $dest) {\n" + block + "\n}\n",
    "record_php": "if (UPLOAD_BOARD && $md5 ) {" + record + "\n}\n",
    "retention_php": next(line.strip() for line in texts["admin.php"].splitlines() if 'mysql_board_call( "DELETE FROM f_md5 ' in line) + "\n",
    "database_mapping_php": texts["yotsuba_config.php"].split("if( $using_pdo ) {", 1)[1].split("load_board_config", 1)[0],
    "settings": settings,
    "boundaries": [
        "Only the pinned duplicate gate executes; no original application or credentials are loaded.",
        "Owned database rows model query scopes and the strict one-day history predicate.",
        "The source's LIMIT 1 query has no ORDER BY; a particular duplicate-link choice is not guaranteed.",
        "Actor cooldowns, exact-file blacklist and Robot9000 are separate predicates.",
    ],
}
path = Path("fixtures/image-duplicate-source.json")
encoded = json.dumps(result, indent=2) + "\n"
if args.check:
    assert path.read_text() == encoded
else:
    path.write_text(encoded)
