#!/usr/bin/env python3
"""Check direct grouped native commands in the Windows CI steps.

GitHub's pwsh footer propagates only the final LASTEXITCODE. This is a check of
our literal workflow command groups, not a general PowerShell parser.
"""
from pathlib import Path
import re
import sys

GUARD = "if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }"
NATIVE = re.compile(r"^(?:npm|npx|cargo|rustup|node|python3?|gh|git)(?:\s|$)")


def missing_guards(source):
    missing = []
    for step in re.split(r"(?m)(?=^      - )", source):
        lines = step.splitlines()
        if "        shell: pwsh" not in lines:
            continue
        for start, line in enumerate(lines):
            if not re.fullmatch(r"        run: \|[-+]?", line):
                continue
            commands = []
            for body in lines[start + 1:]:
                if body and not body.startswith("          "):
                    break
                body = body.strip()
                if body and not body.startswith("#"):
                    commands.append(body)
            native = [i for i, command in enumerate(commands) if NATIVE.match(command)]
            for index in native[:-1]:
                if commands[index + 1] != GUARD:
                    missing.append(commands[index])
    return missing


def self_test():
    def block(shell, body):
        return "      - name: Fixture\n        shell: " + shell + "\n        run: |\n" + "".join(
            "          " + command + "\n" for command in body
        )
    unguarded = block("pwsh", ["cargo test --locked", "node --test fixture.mjs"])
    assert missing_guards(unguarded) == ["cargo test --locked"]
    assert not missing_guards(block("pwsh", ["cargo test --locked", "# comment", GUARD, "node --test fixture.mjs"]))
    assert not missing_guards(block("bash", ["cargo test --locked", "node --test fixture.mjs"]))
    single = block("pwsh", ["cargo test --locked"])
    assert not missing_guards(single + single)
    assert missing_guards(block("pwsh", ["npm ci --ignore-scripts", "npx playwright install chromium"])) == ["npm ci --ignore-scripts"]


if __name__ == "__main__":
    self_test()
    path = Path(sys.argv[1]) if len(sys.argv) == 2 else Path(__file__).resolve().parents[1] / ".github/workflows/ci.yml"
    failures = missing_guards(path.read_text())
    if failures:
        for command in failures:
            print(f"Missing immediate Windows exit check: {command}", file=sys.stderr)
        sys.exit(1)
    print("Windows grouped native commands preserve earlier failures.")
