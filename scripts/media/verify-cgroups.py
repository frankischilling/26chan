#!/usr/bin/env python3
"""Service-side resource gate. Execute the jailer only after kernel readback."""
import os
import pathlib
import re
import sys


def resource_groups(unit, membership, mountinfo):
    if re.fullmatch(r'26chan-media-[0-9a-f]{32}\.service', unit) is None:
        raise ValueError('unexpected unit')
    expected = '/system.slice/' + unit
    groups = {}
    for line in membership.splitlines():
        _, controllers, path = line.split(':', 2)
        for controller in controllers.split(','):
            groups[controller] = path
    required = ('memory', 'cpu', 'pids')
    legacy = any(controller in groups for controller in required)
    required = required if legacy else ('',)
    if any(groups.get(controller) != expected for controller in required):
        raise ValueError('required controllers must contain this generated unit')
    mounts = {}
    for line in mountinfo.splitlines():
        before, after = line.split(' - ', 1)
        fields = before.split()
        kind, _, options = after.split()
        # Reject remounted subtrees and escaped/nonstandard mount paths. This
        # operator profile requires the host hierarchy, not a cgroup namespace.
        if fields[3] != '/' or '\\' in fields[4]:
            continue
        if kind == 'cgroup' and legacy:
            for controller in options.split(','):
                if controller in required:
                    if controller in mounts:
                        raise ValueError('ambiguous controller mount')
                    mounts[controller] = pathlib.Path(fields[4])
        elif kind == 'cgroup2' and not legacy:
            if '' in mounts:
                raise ValueError('ambiguous unified mount')
            mounts[''] = pathlib.Path(fields[4])
    if any(controller not in mounts for controller in required):
        raise ValueError('required controller mount missing')
    return {controller: mounts[controller] / expected.lstrip('/') for controller in required}


def enforce_limits(groups):
    def read(directory, name):
        return (directory / name).read_text().strip()

    def require(directory, name, value):
        if read(directory, name) != value:
            raise ValueError('ineffective cgroup limit: ' + name)

    if 'memory' in groups:
        memory, cpu, pids = (groups[name] for name in ('memory', 'cpu', 'pids'))
        require(memory, 'memory.limit_in_bytes', '268435456')
        require(memory, 'memory.use_hierarchy', '1')
        # Preflight *all* controls before writing the service's own cgroup.
        int(read(memory, 'memory.memsw.limit_in_bytes'))
        int(read(memory, 'memory.swappiness'))
        quota = int(read(cpu, 'cpu.cfs_quota_us'))
        period = int(read(cpu, 'cpu.cfs_period_us'))
    else:
        memory = cpu = pids = groups['']
        require(memory, 'memory.max', '268435456')
        require(memory, 'memory.swap.max', '0')
        quota, period = map(int, read(cpu, 'cpu.max').split())
    require(pids, 'pids.max', '32')
    if quota <= 0 or period <= 0 or quota != period:
        raise ValueError('ineffective CPU quota')
    if 'memory' in groups:
        # v1 limits combined resident memory + swap; swappiness=0 controls
        # cgroup reclaim. This is NOT v2's absolute memory.swap.max=0 policy.
        for name, value in (('memory.memsw.limit_in_bytes', '268435456'),
                            ('memory.swappiness', '0')):
            # Never create an ordinary file when controller support is absent.
            descriptor = os.open(memory / name, os.O_WRONLY | os.O_TRUNC | os.O_CLOEXEC | os.O_NOFOLLOW)
            with os.fdopen(descriptor, 'w') as control:
                control.write(value + '\n')
            require(memory, name, value)


def main():
    unit, *command = sys.argv[1:]
    if os.geteuid() != 0 or not command or not pathlib.Path(command[0]).is_absolute():
        raise ValueError('root service and absolute jailer command required')
    groups = resource_groups(unit, pathlib.Path('/proc/self/cgroup').read_text(),
                             pathlib.Path('/proc/self/mountinfo').read_text())
    enforce_limits(groups)
    # v1 without properties does not migrate. v2 can migrate even without
    # properties, so explicitly target the SAME verified systemd unit instead
    # of jailer's default /firecracker cgroup (which another operator may own).
    if 'memory' in groups:
        command[1:1] = ['--cgroup-version', '1']
    else:
        command[1:1] = ['--cgroup-version', '2', '--parent-cgroup', 'system.slice/' + unit]
    os.execve(command[0], command, {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C'})


if __name__ == '__main__':
    main()
