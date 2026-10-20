#!/usr/bin/env python3
"""Pin the supplied VIP decision fragments without importing its application."""
import argparse
import hashlib
import json
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
record = {
    "source": {"imgboard.php": digest},
    "lookup_php": "function parse_vip_capcode($capcode) {" + lookup,
    "dispatch_php": "function parse_capcode($capcode, &$name = null)\n{"
    + dispatch
    + "\n  return 'none';\n}\n",
    "markup_php": markup,
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
