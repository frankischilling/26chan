#!/usr/bin/env python3
"""Qualify the bounded cookie subset against an owned loopback PHP server.

Usage: python3 extract-flat-cookie-reference.py PHP OUTPUT [--check]
The caller must verify the PHP executable comes from the official package.
This script checks runtime versions and word size, not binary provenance.
No reference application is loaded and no external destination is contacted.
"""
import argparse
import http.client
import json
import pathlib
import socket
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("php")
parser.add_argument("output", type=pathlib.Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
php = pathlib.Path(args.php).resolve(strict=True)
if not args.output.parent.is_dir() or args.output.resolve() == php:
    raise SystemExit("Use an existing output directory and a distinct output file.")
probe = '''<?php
header('Content-Type: application/json');
$entries=[];
foreach ($_COOKIE as $name=>$value) { $entries[]=[(string)$name,$value]; }
echo json_encode(['php_version'=>PHP_VERSION,'php_int_size'=>PHP_INT_SIZE,
'pcre_version'=>PCRE_VERSION,'sapi'=>PHP_SAPI,'raw'=>$_SERVER['HTTP_COOKIE']??null,
'cookies'=>$entries], JSON_THROW_ON_ERROR);
'''
cases = [(None, True), ("", True)]
for name in ["a", "_tcs", "cf_clearance", "__Host-board_session", "Case_123-name"]:
    for value in ["", "0", "abc123", "x.America/New_York.999.33", "a=b", "!#$&'()*-./:<=>?@[]^_`{|}~"]:
        cases.append((name + "=" + value, True))
cases.extend([
    ("a=1; b=2", True), ("a=1;b=2", True),
    ("_tcs=x.UTC.999.33; cf_clearance=; __Host-board_session=abc", True),
])
for value in ["a=1; a=2", "a.b=1", "a b=1", "a[]=1", "0=1", 'a="quoted"',
              "a=x+y", "a=x%20y", "a=x,y", "a=x\\y", "a=1;", ";a=1",
              "a=1;  b=2", "a=1\t", "a", "=1", "a=1 ", "a=%00"]:
    cases.append((value, False))
results = []
with tempfile.TemporaryDirectory(prefix="board-cookie-qualification-") as root:
    path = pathlib.Path(root)
    (path / "probe.php").write_text(probe)
    # Bind only loopback. A raced port fails startup rather than changing hosts.
    with socket.socket() as reserved:
        reserved.bind(("127.0.0.1", 0))
        port = reserved.getsockname()[1]
    process = subprocess.Popen([str(php), "-n", "-S", f"127.0.0.1:{port}", "-t", root],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        deadline = time.monotonic() + 5
        while True:
            if process.poll() is not None:
                raise RuntimeError("Owned PHP server exited during startup.")
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                    break
            except OSError:
                if time.monotonic() >= deadline:
                    raise RuntimeError("Owned PHP server startup timed out.")
                time.sleep(0.02)
        for raw, accepted in cases:
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
            try:
                headers = {} if raw is None else {"Cookie": raw}
                connection.request("GET", "/probe.php", headers=headers)
                response = connection.getresponse()
                data = json.loads(response.read(16384))
                if response.status != 200 or data["php_version"] != "8.4.24" or data["php_int_size"] != 8 or data["pcre_version"] != "10.46 2025-08-27" or data["sapi"] != "cli-server":
                    raise RuntimeError("Cookie reference runtime differs.")
                if data["raw"] != raw:
                    raise RuntimeError("The PHP HTTP parser changed the synthetic Cookie header.")
                results.append({"raw": raw, "accepted_subset": accepted, "source_raw": data["raw"], "cookies": data["cookies"]})
            finally:
                connection.close()
    finally:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)
output = json.dumps({"php_version": "8.4.24", "php_int_size": 8,
                     "pcre_version": "10.46 2025-08-27", "sapi": "cli-server", "cases": results},
                    indent=2, ensure_ascii=True) + "\n"
if args.check:
    if args.output.read_text() != output:
        raise SystemExit("Flat-cookie source fixture differs.")
else:
    args.output.write_text(output)
print(f"{len(results)} actual loopback PHP cookie cases.")
