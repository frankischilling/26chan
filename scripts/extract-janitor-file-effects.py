#!/usr/bin/env python3
"""Pin isolated request file-policy branches; never load the source application."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
inputs = {
    "admin.php": "4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb",
    "reports/ReportQueue.php": "e1eaa54bd229f0d718d026cb11a757576f67f02a86e21c4bf5ff051bbfbac0d1",
}
texts = {}
for filename, digest in inputs.items():
    raw = (args.source / filename).read_bytes()
    if hashlib.sha256(raw).hexdigest() != digest:
        raise SystemExit(f"Audited source differs: {filename}")
    texts[filename] = raw.decode().replace("\r\n", "\n")


def between(text, start, end):
    if text.count(start) != 1:
        raise SystemExit(f"Expected one source boundary: {start}")
    result = start + text.split(start, 1)[1].split(end, 1)[0]
    if len(result) > 8192:
        raise SystemExit("File policy branch exceeds its bound")
    return result


admin = texts["admin.php"].split("function adminBanReq()", 1)[1]
review = texts["reports/ReportQueue.php"].split("  public function accept_ban_request() {", 1)[1].split("  public function deny_ban_request() {", 1)[0]
clear = texts["reports/ReportQueue.php"].split("  private function clear_ban_request(", 1)[1].split("    // Janitor stats", 1)[0]
record = {
    "sources": inputs,
    "scope": "Creation copy policy, amended JSON-only file removal, image blacklist insertion and request-clear file removal. File access and SQL calls are intercepted using synthetic records.",
    "php": {
        "capture": between(admin, "\t\t// Make sure we don't have any illegal reports", "\t\tif ($post['resto'])"),
        "amend": between(review, "    // Delete the file if the request was amended", "    if ($req_amend && isset($_POST['public_reason'])"),
        "blacklist": between(review, "    // Blacklisting", "    // Janitor stats"),
        "clear": between(clear, "    @unlink($ban_image_dir", "    // Janitor stats"),
    },
}
output = Path(__file__).resolve().parent.parent / "fixtures/janitor-file-effects-source.json"
encoded = json.dumps(record, indent=2) + "\n"
if args.check:
    if output.read_text() != encoded:
        raise SystemExit("Janitor file-effect source differs")
else:
    output.write_text(encoded)
print("Janitor file-effect branches match the pinned source.")
