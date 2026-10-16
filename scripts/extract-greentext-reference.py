#!/usr/bin/env python3
"""Evaluate the pinned greentext pass on synthetic, preformatted text."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("output", type=Path)
parser.add_argument("--php", default="php")
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
source = (args.source / "imgboard.php").read_bytes()
expected = "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445"
if hashlib.sha256(source).hexdigest() != expected:
    raise SystemExit("Pinned imgboard.php differs")
text = source.decode("utf-8")
start = text.index("\t$com = preg_replace( '#(&gt;&gt;&gt;/")
end = text.index("\t$admin_style =", start)
block = text[start:end]
if len(block) > 2048:
    raise SystemExit("Selected greentext block exceeds bounds")
inputs = [">green", ">>not-a-post", ">>>not-a-board", ">>0", ">>42tail", ">>>42", ">>>>42",
          ">>>/unknown/", ">>>/unknown/term", ">>>/G/42", ">>>/unknown", ">>>//42",
          "> >tail", ">>>>/unknown/42", ">>>/" + "x"*30 + "/42", ">>>/" + "x"*31 + "/42"]
cases = []
for prefix in ("", "text\n", "text\n ", "text\n  "):
    for value in inputs:
        raw = prefix + value
        # These synthetic cases contain no static link, BBCode or entities.
        # Feed the greentext pass a separately stated 35-scalar wordwrap shape.
        lines = []
        for line in raw.split("\n"):
            words = []
            for word in line.split(" "):
                wrapped = "".join(ch + ("{{w_br}}" if (i+1)%35 == 0 else "") for i,ch in enumerate(word))
                words.append(html.escape(wrapped, quote=False))
            lines.append(" ".join(words))
        cases.append({"input": raw, "preformatted": "<br>".join(lines)})
program = "<?php error_reporting(E_ALL); $cases=json_decode(" + json.dumps(json.dumps(cases)) + ",true,32,JSON_THROW_ON_ERROR);\n"
program += "foreach($cases as &$case) { $com=$case['preformatted'];\n" + block + "\n$case['output']=$com; } echo json_encode($cases,JSON_THROW_ON_ERROR);"
result = subprocess.run([args.php, "-d", "memory_limit=32M", "-d", "max_execution_time=5"],
                        input=program, text=True, capture_output=True, timeout=10, check=True)
if result.stderr or len(result.stdout) > 131072:
    raise SystemExit("Greentext worker exceeded its output contract")
output = json.dumps({"source": "imgboard.php", "sha256": expected,
                     "scope": "Exact greentext pass after synthetic 35-scalar wrapping and line breaks; no linkification or BBCode",
                     "cases": json.loads(result.stdout)}, ensure_ascii=False, indent=2) + "\n"
if args.check:
    if args.output.read_text() != output: raise SystemExit("Greentext fixture differs")
else:
    args.output.write_text(output)
print(f"Source greentext cases: {len(cases)}")
