#!/usr/bin/env python3
"""Pin the supplied VIP decision fragments without importing its application."""
import argparse
import hashlib
import json
import re
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
digest = "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445"
raw = (args.source / "imgboard.php").read_bytes()
assert hashlib.sha256(raw).hexdigest() == digest
text = raw.decode().replace("\r\n", "\n")
lookup = text.split("function parse_vip_capcode($capcode) {", 1)[1].split(
    "\nfunction parse_capcode(", 1
)[0]
dispatch = text.split("function parse_capcode($capcode, &$name = null)\n{", 1)[1].split(
    "\n\tif (!has_level())", 1
)[0]
markup = text.split("\t\tcase 'verified':", 1)[1].split("\n\t\tdefault:", 1)[0]
styles = {}
for name, expected in {
    "futabanew": "8beb159cfcf83d042a68b1037997511c7b0bb959289eb1b5757036cdd7aa0945",
    "yotsubanew": "05d61b9a886aca60bd418b63a5b86d2ef3050d1d90c640da64f2206dda740db9",
    "tomorrow": "40b4c474a93e87737811c77a50cce9af4fadc9b025a3863e126ca418a8f33a4a",
    "photon": "a4741759760d959b4a2e29d9622ab63a91d1cdc49f10679335b71f5b16e9575d",
    "0ch": "a2ab0e9aa588564cfdf81d997cc82492749894f723e6ca6969e0aa419607b333",
    "spooky2017": "010e148d0070d97f9fba04d9e54a302af007a09151d92ca39f5b439e5d7d1690",
}.items():
    raw = (args.source / "css" / f"{name}.css").read_bytes()
    assert hashlib.sha256(raw).hexdigest() == expected
    css = raw.decode().replace("\r\n", "\n")
    rules = re.findall(r"^.*capcodeVerified[^\n]*\{\s*color:\s*#007FFF\s*!important;\s*\}", css, re.M)
    assert len(rules) == 1
    styles[f"css/{name}.css"] = {"sha256": expected, "rule": rules[0]}
record = {
    "source": {"imgboard.php": digest},
    "lookup_php": "function parse_vip_capcode($capcode) {" + lookup,
    "dispatch_php": "function parse_capcode($capcode, &$name = null)\n{"
    + dispatch
    + "\n  return 'none';\n}\n",
    "markup_php": markup,
    "styles": styles,
    "boundaries": [
        "Owned documentation IPv4 peer and fixed request timestamp.",
        "Database calls are replaced with explicit synthetic rows and recorded effects.",
        "Only the VIP branch of parse_capcode is retained; staff branches are not executed.",
        "Caller supplies the name reference only when CAPTCHA bypass is true.",
        "Passwords and hashes are synthetic; no source configuration is loaded.",
    ],
}
output = Path("fixtures/vip-capcode-source.json")
encoded = json.dumps(record, indent=2) + "\n"
if args.check:
    assert output.read_text() == encoded
else:
    output.write_text(encoded)
