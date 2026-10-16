#!/usr/bin/env python3
"""Run only pinned pure email methods against owned strings; never the app."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE_SHA = "f646724efd291744fc8b4e46fda4f67c069277842dd7798512f864142f18cda2"
METHOD_SHAS = [
    "964b394d8d579d2462d51985c24a1ff4bc1a338608641a5cf9ac819e5ba27938",
    "bb42ed3f60707e9f17d0b2f9e414d814bc7e9b8bbf1d04e4bc637bae738788c5",
    "345959ba0cf251a8b3f7a0bb0a6e650f0ea6ca51067071e695a4514a7d24c4d6",
]
DOMAIN_SHA = "14067c37f08cd039edab5a4384f6c2ec9b47a9ea0ecb7fb5a1227abf6ad2c02f"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--php", default="php")
    parser.add_argument("--source", type=Path)
    args = parser.parse_args()
    raw = (ROOT / "fixtures/email-verification-reference.json").read_bytes()
    assert len(raw) <= 32768, "Email reference exceeds its bound."
    fixture = json.loads(raw)
    assert fixture["source_sha256"] == SOURCE_SHA
    methods = fixture["methods"]
    assert len(methods) == 3
    assert [hashlib.sha256(m.encode()).hexdigest() for m in methods] == METHOD_SHAS
    block = fixture["domain_block"]
    assert hashlib.sha256(block.encode()).hexdigest() == DOMAIN_SHA
    if args.source:
        source = (args.source / "signin.php").read_bytes()
        assert hashlib.sha256(source).hexdigest() == SOURCE_SHA
        normalized = source.decode().replace("\r\n", "\n")
        assert all(method in normalized for method in methods) and block in normalized
    cases = fixture["cases"]
    assert len(cases) == 18 and all(len(case["raw"]) <= 320 for case in cases)
    program = """<?php
class OwnedEmailReference {
const VERIFY_EMAIL_DOMAIN=true;
const ERR_BAD_EMAIL='Invalid email address.';
const ERR_BAD_EMAIL_PLUS='Email subaddressing is not allowed';
const ERR_BAD_EMAIL_DOMAIN='This email provider is not allowed.';
private function error($message) { throw new Exception($message); }
""" + block + "\n" + "".join(methods) + """
public function inspect($raw) {
    $email=strtolower(trim($raw));
    try { $this->validate_email($email); }
    catch (Exception $error) { return ['error'=>$error->getMessage()]; }
    $identity=$this->get_domain($email)==='gmail.com'
        ? $this->normalize_gmail_address($email) : $email;
    return ['email'=>$email,'identity_email'=>$identity];
}
}
$reference=new OwnedEmailReference();
$inputs=json_decode(stream_get_contents(STDIN),true,16,JSON_THROW_ON_ERROR);
$output=[];
foreach($inputs as $input) { $output[]=$reference->inspect($input); }
echo json_encode($output,JSON_THROW_ON_ERROR);
"""
    with tempfile.TemporaryDirectory(prefix="owned-email-reference-") as directory:
        path = Path(directory) / "oracle.php"
        path.write_text(program)
        result = subprocess.run(
            [args.php, "-n", "-d", "allow_url_fopen=0", "-d", "allow_url_include=0",
             "-d", "memory_limit=32M", str(path)],
            input=json.dumps([case["raw"] for case in cases]), text=True,
            capture_output=True, check=True, timeout=10,
        )
    observed = json.loads(result.stdout)
    assert observed == [case["expected"] for case in cases], "Email reference differs."
    print("18 pinned email validation/normalization cases passed. No mail or service was invoked.")


if __name__ == "__main__":
    main()
