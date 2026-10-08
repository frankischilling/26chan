#!/usr/bin/env python3
"""Select one compiled native qualification test, never discovery-only output."""
import json
import os
from pathlib import Path
import re
import sys


def select_artifact(data, repository, target):
    if len(data) > 8 * 1024 * 1024:
        raise ValueError('compiler metadata exceeds bound')
    repository, target = Path(repository).resolve(), Path(target).resolve()
    source = repository / 'apps/media-admin/tests/paired_vm.rs'
    found = []
    for line in data.decode('utf-8').splitlines():
        record = json.loads(line)
        if record.get('reason') != 'compiler-artifact':
            continue
        unit = record.get('target', {})
        if unit.get('name') != 'paired_vm' or unit.get('kind') != ['test']:
            continue
        if record.get('profile', {}).get('test') is not True or Path(unit.get('src_path', '')).resolve() != source:
            raise ValueError('unexpected qualification test identity')
        value = record.get('executable')
        if not isinstance(value, str) or '\n' in value or '\r' in value:
            raise ValueError('qualification executable is missing or invalid')
        path = Path(value)
        if (not path.is_absolute() or path.is_symlink() or not path.is_file()
                or not os.access(path, os.X_OK) or not path.resolve().is_relative_to(target)
                or not re.fullmatch(r'paired_vm-[0-9a-f]+', path.name)):
            raise ValueError('qualification executable is outside the owned target')
        found.append(str(path.resolve()))
    if len(found) != 1:
        raise ValueError('expected exactly one compiled qualification test')
    return found[0]


def main():
    if len(sys.argv) != 4:
        raise ValueError('usage: paired-coordinator-artifact.py metadata repository target')
    with open(sys.argv[1], 'rb') as source:
        data = source.read(8 * 1024 * 1024 + 1)
    print(select_artifact(data, sys.argv[2], sys.argv[3]))


if __name__ == '__main__':
    main()
