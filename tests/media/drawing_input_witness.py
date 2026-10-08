"""Private quarantine evidence, independent of board/actor ownership evidence."""
import os
import re
import stat


def private_inputs(root, quarantine, uid):
    assert root.is_absolute() and root.resolve(strict=True) == root
    assert re.fullmatch(r'26chan-dispatch-[a-z0-9_]{8}', root.name)
    assert quarantine == root / 'quarantine'
    info = quarantine.lstat()
    assert stat.S_ISDIR(info.st_mode) and info.st_uid == uid and not info.st_mode & 0o077
    return {name for name in os.listdir(quarantine) if re.fullmatch('[a-f0-9]{32}\\.input', name)}


def private_input(root, quarantine, uid, job):
    assert re.fullmatch('[a-f0-9]{32}', job)
    names = private_inputs(root, quarantine, uid)
    name = job + '.input'
    assert name in names, 'drawing input is absent from the owned quarantine'
    directory = os.open(quarantine, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
        try:
            info = os.fstat(descriptor)
            assert stat.S_ISREG(info.st_mode) and info.st_uid == uid
            assert not info.st_mode & 0o077 and info.st_nlink == 1
            assert 33 <= info.st_size <= 8388608
            return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns)
        finally:
            os.close(descriptor)
    finally:
        os.close(directory)
