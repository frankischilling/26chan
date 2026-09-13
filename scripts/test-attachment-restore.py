#!/usr/bin/env python3
"""Owned PostgreSQL 16 dump + normalized media restore exercise, not deployment."""
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import sys
from urllib.parse import urlsplit, urlunsplit, unquote

REPO = Path(__file__).resolve().parent.parent
ROLES = {
    'MIGRATION_DATABASE_URL': 'board_migrator',
    'TEST_PUBLIC_DATABASE_URL': 'board_public',
    'MEDIA_DATABASE_URL': 'board_media',
    'MEDIA_READ_DATABASE_URL': 'board_media_read',
    'INTAKE_DATABASE_URL': 'board_media_intake',
}
SAFE = {key: os.environ[key] for key in ('PATH', 'SystemRoot', 'WINDIR', 'TEMP', 'TMP', 'LANG') if key in os.environ}
SAFE['PGCONNECT_TIMEOUT'] = '5'
SAFE['PGOPTIONS'] = '-c statement_timeout=30000 -c lock_timeout=5000'


def run(args, env, label, log=None, stdin=None, success=True):
    result = subprocess.run([str(a) for a in args], input=stdin, env=env, capture_output=True, timeout=60)
    if log is not None:
        log.write_bytes(result.stdout + result.stderr)
    if (result.returncode == 0) != success:
        raise RuntimeError(label + ' failed; inspect the private exercise directory')
    return result.stdout.decode().strip()


def main():
    os.chdir(REPO)
    os.umask(0o077)
    windows = os.name == 'nt'
    executable_suffix = '.exe' if windows else ''
    fixture = REPO / ('target/debug/examples/attachment_restore' + executable_suffix)
    migrator = REPO / ('target/debug/board-migrate' + executable_suffix)
    assert fixture.is_file() and migrator.is_file(), 'Build the restore example and board-migrate first'
    if windows:
        owner_root = REPO / '.local/intake-postgres'
        marker = json.loads((owner_root / 'owned-cluster.json').read_text())
        cluster = Path(marker['cluster']).resolve()
        assert cluster.parent == owner_root.resolve() and re.fullmatch('cluster-[0-9a-f]{32}', cluster.name)
        owned = json.loads((cluster / 'state.json').read_text())
        data = cluster / 'data'
        assert Path(owned['data']).resolve() == data.resolve()
        port = int(owned['port'])
        pg_bin = owner_root / 'pgsql/bin'
        admin_env = {**SAFE, 'PGHOST': '127.0.0.1', 'PGPORT': str(port), 'PGUSER': 'postgres', 'PGDATABASE': 'postgres', 'PGPASSWORD': owned['password']}
        admin_prefix = []
        scratch_parent = cluster  # Existing private Windows ACLs protect receipts/backups.
    else:
        assert os.geteuid() == 0, 'Use the owned Linux wrapper as root'
        cluster = Path((REPO / '.local/cluster-path').read_text().strip())
        assert re.fullmatch(r'/tmp/board-postgres\.[A-Za-z0-9]+', str(cluster))
        assert cluster.is_dir() and not cluster.is_symlink()
        data = cluster
        port = 55432
        pg_bin = Path('/usr/lib/postgresql/16/bin')
        admin_env = {**SAFE, 'PGHOST': '/tmp', 'PGPORT': str(port), 'PGUSER': 'postgres', 'PGDATABASE': 'postgres'}
        admin_prefix = ['runuser', '-u', 'postgres', '--']
        scratch_parent = Path('/tmp')
    psql = pg_bin / ('psql' + executable_suffix)

    def admin(statement, database='postgres', log=None):
        return run([*admin_prefix, psql, '-XAt', '-v', 'ON_ERROR_STOP=1', '-c', statement],
                   {**admin_env, 'PGDATABASE': database}, 'bootstrap SQL', log)

    assert Path(admin('SHOW data_directory')).resolve() == data.resolve(), 'Port is not the owned cluster'
    assert admin("SELECT current_setting('server_version_num')::int/10000") == '16'
    urls = {}
    for key, role in ROLES.items():
        parsed = urlsplit(os.environ[key])
        assert parsed.scheme in ('postgres', 'postgresql') and parsed.hostname == '127.0.0.1'
        assert parsed.port == port and unquote(parsed.username or '') == role
        assert parsed.password and not parsed.query and not parsed.fragment
        urls[key] = parsed

    def role_env(key, database):
        parsed = urls[key]
        return {**SAFE, 'PGHOST': '127.0.0.1', 'PGPORT': str(port), 'PGUSER': ROLES[key],
                'PGPASSWORD': unquote(parsed.password), 'PGDATABASE': database}

    token = secrets.token_hex(12)
    names = ['imageboard_attachment_' + token + suffix for suffix in ('_source', '_restore')]
    root = scratch_parent / ('attachment-restore-' + token)
    root.mkdir(mode=0o700)
    created = []
    print('Owned attachment restore exercise started.', flush=True)
    try:
        for name in names:
            assert re.fullmatch('imageboard_attachment_[0-9a-f]{24}_(source|restore)', name)
            admin(f'CREATE DATABASE {name} OWNER board_migrator')
            created.append(name)
            admin(f'REVOKE ALL ON DATABASE {name} FROM PUBLIC; GRANT CONNECT ON DATABASE {name} TO ' + ','.join(ROLES.values()))
        source, restored = names

        def fixture_env(database):
            return {**SAFE, 'ATTACHMENT_RESTORE_FIXTURE': 'owned-disposable', **{
                key: urlunsplit((url.scheme, url.netloc, '/' + database, '', '')) for key, url in urls.items()}}

        run([migrator], {**SAFE, 'MIGRATION_DATABASE_URL': fixture_env(source)['MIGRATION_DATABASE_URL']}, 'source migrations', root / 'migrate.log')
        source_root, backup_root, restore_root = (root / name for name in ('source', 'backup', 'restored'))
        for directory in (source_root, backup_root, restore_root):
            directory.mkdir(mode=0o700)
        run([fixture, 'create', source_root], fixture_env(source), 'fixture creation', root / 'create.log')

        def fingerprint(database):
            tables = {'content.posts': 'id', 'content.threads': 'id', 'content.post_media': 'post_id',
                      'content.media_clock': 'singleton', 'media.assets': 'id', 'media.jobs': 'id',
                      'media_intake.handles': 'job_id', 'public._sqlx_migrations': 'version'}
            values = []
            for table, order in tables.items():
                statement = f"SELECT coalesce(md5(string_agg(row_to_json(r)::text,'' ORDER BY {order})), '') FROM {table} r"
                values.append(run([psql, '-XAt', '-v', 'ON_ERROR_STOP=1', '-c', statement], role_env('MIGRATION_DATABASE_URL', database), 'data fingerprint'))
            return values

        before = fingerprint(source)
        run([pg_bin / ('pg_dump' + executable_suffix), '--format=custom', '--file', backup_root / 'database.dump'],
            role_env('MIGRATION_DATABASE_URL', source), 'database dump', root / 'dump.log')

        def copy_outputs(start, destination):
            destination.mkdir(mode=0o700)
            entries = list(start.iterdir())
            assert len(entries) <= 20
            for path in entries:
                if path.name == '.publication.lock':
                    continue  # A restored publisher creates its own permanent lock.
                assert re.fullmatch('[0-9a-f]{32}(?:\\.thumb)?\\.png', path.name)
                assert path.is_file() and not path.is_symlink() and path.stat().st_size <= 5_242_880
                shutil.copyfile(path, destination / path.name)

        # Source is quiescent: the only fixture writer exited before the dump.
        copy_outputs(source_root / 'objects', backup_root / 'objects')
        shutil.copyfile(source_root / 'manifest.json', backup_root / 'manifest.json')
        with (backup_root / 'database.dump').open('rb') as stream:
            result = subprocess.run([*admin_prefix, str(pg_bin / ('pg_restore' + executable_suffix)), '--dbname', restored,
                                     '--single-transaction', '--exit-on-error'], stdin=stream, env=admin_env,
                                    capture_output=True, timeout=60)
        (root / 'restore.log').write_bytes(result.stdout + result.stderr)
        assert result.returncode == 0, 'Atomic restore failed; inspect private restore.log'
        assert fingerprint(restored) == before, 'Restored attachment, job, capability, clock or migration data differs'
        copy_outputs(backup_root / 'objects', restore_root / 'objects')
        shutil.copyfile(backup_root / 'manifest.json', restore_root / 'manifest.json')
        # Negative controls use only the restored copy. The same verification
        # command must reject missing or corrupt media, then pass after repair.
        probe = next((restore_root / 'objects').iterdir())
        withheld = restore_root / 'withheld.png'
        probe.rename(withheld)
        run([fixture, 'verify', restore_root], fixture_env(restored), 'missing-file rejection', root / 'missing.log', success=False)
        withheld.rename(probe)
        original = probe.read_bytes()
        probe.write_bytes(bytes([original[0] ^ 1]) + original[1:])
        run([fixture, 'verify', restore_root], fixture_env(restored), 'corrupt-file rejection', root / 'corrupt.log', success=False)
        shutil.copyfile(backup_root / 'objects' / probe.name, probe)
        run([fixture, 'verify', restore_root], fixture_env(restored), 'restored reads and cleanup', root / 'verify.log')
        assert fingerprint(source) == before, 'Restored cleanup or posting changed the source database'
        for path in (backup_root / 'objects').iterdir():
            expected = hashlib.sha256(path.read_bytes()).digest()
            assert hashlib.sha256((source_root / 'objects' / path.name).read_bytes()).digest() == expected
        print('PASS dump and file restore: seven attachments, fourteen files, exact JSON/bytes, four URL forms, deletion and archive visibility, one-use receipts, interrupted cleanup, monotonic numbering and unchanged source.', flush=True)
        for name in reversed(created):
            assert admin(f"SELECT pg_get_userbyid(datdba) FROM pg_database WHERE datname='{name}'") == 'board_migrator'
            admin(f'DROP DATABASE {name}')
        # The only recursive removal targets this newly generated, validated
        # private fixture directory. No source workspace or existing DB is removed.
        assert root.resolve().parent == scratch_parent.resolve() and root.name == 'attachment-restore-' + token
        assert not root.is_symlink()
        shutil.rmtree(root)
        print('Disposable source/restore databases and private fixture files removed.', flush=True)
    except BaseException:
        print(f'Exercise failed; private fixtures retained at {root}.', file=sys.stderr)
        raise


if __name__ == '__main__':
    main()
