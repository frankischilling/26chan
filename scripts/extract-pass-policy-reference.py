#!/usr/bin/env python3
"""Extract only Pass state decisions, without source SQL or credential handling."""
import argparse, hashlib, json
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('source',type=Path);p.add_argument('--check',action='store_true');a=p.parse_args()
inputs={'auth.php':'b300f64e6d2bc4306fa473b3a509fe38e12c292f4e6b6879c4ccf384a9ab6735','lib/auth.php':'98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019'}
bodies={}
for name,digest in inputs.items():
 raw=(a.source/name).read_bytes();assert hashlib.sha256(raw).hexdigest()==digest
 bodies[name]=raw.decode().replace('\r\n','\n')
login=bodies['auth.php'].split("    $last_used = strtotime($pass['last_used']);",1)[1].split('    // Update country',1)[0]
login="    $last_used = strtotime($pass['last_used']);"+login
posting=bodies['lib/auth.php'].split("\t\tif ((int)$res['expiration_date'] <= $time)",1)[1].split("\t\t$update_country = '';",1)[0]
posting="\t\tif ((int)$res['expiration_date'] <= $time)"+posting
record={'sources':inputs,'login_php':login.strip(),'posting_php':posting.strip(),'login_timeout':900,'posting_timeout':1800,'boundary_stubs':['synthetic status and timestamps','owned documentation IPv4 addresses','error returns instead of HTTP termination','activation reports a transition instead of issuing SQL','no cookie writes']}
out=Path('fixtures/pass-policy-source.json');encoded=json.dumps(record,indent=2)+'\n'
if a.check: assert out.read_text()==encoded
else: out.write_text(encoded)
