"""Check actual Cargo normal/build dependency paths, including enabled features."""
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
RUNTIMES = (
    'board-public', 'board-staff', 'board-media-admin', 'board-media-http',
    'board-media-intake', 'board-media-dispatch', 'board-monitor',
    'board-resource-monitor', 'board-maintenance-monitor',
)
PARSERS = {'board-media-guest', 'zune-jpeg', 'zune-core', 'jpeg-encoder'}


def reachable(start, nodes):
    seen = set()
    pending = [start]
    while pending:
        node = pending.pop()
        if node in seen:
            continue
        seen.add(node)
        pending.extend(edge['pkg'] for edge in nodes[node]['deps']
                       if any(kind['kind'] in (None, 'build') for kind in edge['dep_kinds']))
    return seen


def main():
    result = subprocess.run(
        ['cargo', 'metadata', '--locked', '--all-features', '--format-version', '1'],
        cwd=ROOT, capture_output=True, text=True, timeout=60, check=True)
    metadata = json.loads(result.stdout)
    packages = {package['id']: package['name'] for package in metadata['packages']}
    nodes = {node['id']: node for node in metadata['resolve']['nodes']}
    workspace = {packages[package]: package for package in metadata['workspace_members']}
    for runtime in RUNTIMES:
        found = {packages[package] for package in reachable(workspace[runtime], nodes)} & PARSERS
        if found:
            raise RuntimeError(f'{runtime} links prohibited guest parsing dependencies: {sorted(found)}')
    guest = workspace['board-media-guest']
    if 'zune-jpeg' not in {packages[package] for package in reachable(guest, nodes)}:
        raise RuntimeError('healthy guest JPEG dependency is missing')
    # Mutate only this in-memory graph, not Cargo manifests or files. Prove the
    # same traversal detects a forbidden normal dependency from a real runtime.
    public = workspace['board-public']
    control = dict(nodes)
    control[public] = {**nodes[public], 'deps': [*nodes[public]['deps'],
        {'pkg': guest, 'dep_kinds': [{'kind': None, 'target': None}]}]}
    if 'zune-jpeg' not in {packages[package] for package in reachable(public, control)}:
        raise RuntimeError('injected guest dependency was not detected')
    print('PASS credentialed runtime dependency graphs exclude JPEG parsers; guest control and injected-edge rejection passed')


if __name__ == '__main__':
    main()
