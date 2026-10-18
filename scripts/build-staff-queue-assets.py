#!/usr/bin/env python3
"""Keep source staff styles and fixed-origin assets reproducible."""
import argparse
import base64
import hashlib
import json
import re
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--check", action="store_true")
parser.add_argument("--source", type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
record = json.loads((root / "docs/staff-queue-assets.json").read_text())
source = (root / "fixtures/staff-reportqueue-source.css").read_bytes()
expected = "f547b21681c2063cc8c92734b2e8c298f31d9c8c3667543864a7d19f988b67ae"
if hashlib.sha256(source).hexdigest() != expected or record["sha256"] != expected:
    raise SystemExit("Staff stylesheet source differs")
if args.source and (args.source / "reports/css/reportqueue.css").read_bytes() != source:
    raise SystemExit("Supplied staff stylesheet differs")
css = source.decode().replace("\r\n", "\n")
matches = list(re.finditer(r"data:font/woff2;base64,([A-Za-z0-9+/=]+)", css))
if len(matches) != 1:
    raise SystemExit("Expected exactly one embedded source font")
match = matches[0]
font = base64.b64decode(match[1], validate=True)
css = css[:match.start()] + "/staff-assets/reportqueue.woff2" + css[match.end():]
for old, new in [
    ("/image/select-arrow.png", "/staff-assets/select-arrow.png"),
    ("/image/select-arrow-light.png", "/staff-assets/select-arrow-light.png"),
    ("//s.4cdn.org/image/minileaf.gif", "/staff-assets/minileaf.gif"),
]:
    if css.count(old) != 1:
        raise SystemExit(f"Source asset occurrence differs: {old}")
    css = css.replace(old, new)
for name, data in [("reportqueue.v1.css", css.encode()), ("reportqueue.woff2", font)]:
    path = root / "apps/staff/static" / name
    if args.check:
        if path.read_bytes() != data:
            raise SystemExit(f"Generated staff asset differs: {name}")
    else:
        path.write_bytes(data)
for name in ["select-arrow.png", "select-arrow-light.png", "favicon-team.ico"]:
    data = (root / "apps/staff/static" / name).read_bytes()
    if hashlib.sha256(data).hexdigest() != record["assets"][name]:
        raise SystemExit(f"Imported staff image differs: {name}")
    if args.source and (args.source / "reports/image" / name).read_bytes() != data:
        raise SystemExit(f"Supplied staff image differs: {name}")
print("Staff queue styles, font and fixed image assets match the source.")
