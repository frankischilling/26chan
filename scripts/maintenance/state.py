"""Strict, portable maintenance configuration and journal transitions."""

import json
import posixpath

TARGETS = ('application', 'host', 'media_guest', 'monitoring')
CONFIG_LIMIT = 16384
JOURNAL_LIMIT = 4096
MAX_TIMESTAMP = 2**53
ERROR = 'Invalid maintenance state or configuration'
FIELDS = {'schema', 'target', 'started_ms', 'finished_ms', 'outcome', 'last_success_ms', 'failure_pending'}


def canonical_path(value):
    if (type(value) is not str or not value.startswith('/') or value.startswith('//')
            or '\0' in value or posixpath.normpath(value) != value):
        raise ValueError(ERROR)
    try:
        value.encode('utf-8')
    except UnicodeError:
        raise ValueError(ERROR) from None
    return value


def _json(raw, limit):
    if type(raw) is not bytes or not raw or len(raw) > limit:
        raise ValueError(ERROR)

    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(ERROR)
            result[key] = value
        return result

    def invalid(_value):
        raise ValueError(ERROR)

    try:
        return json.loads(raw.decode('utf-8'), object_pairs_hook=unique, parse_constant=invalid)
    except (ValueError, RecursionError):
        raise ValueError(ERROR) from None


def parse_config(raw):
    value = _json(raw, CONFIG_LIMIT)
    if (type(value) is not dict or set(value) != {'target', 'state_directory', 'command', 'timeout_seconds'}
            or value['target'] not in TARGETS or type(value['timeout_seconds']) is not int
            or not 1 <= value['timeout_seconds'] <= 3600):
        raise ValueError(ERROR)
    canonical_path(value['state_directory'])
    command = value['command']
    if type(command) is not list or not 1 <= len(command) <= 32:
        raise ValueError(ERROR)
    try:
        if any(type(arg) is not str or not arg or '\0' in arg or len(arg.encode('utf-8')) > 1024 for arg in command):
            raise ValueError(ERROR)
    except UnicodeError:
        raise ValueError(ERROR) from None
    canonical_path(command[0])
    return value


def _timestamp(value):
    if type(value) is not int or not 0 < value < MAX_TIMESTAMP:
        raise ValueError(ERROR)


def validate_journal(value, target):
    if (target not in TARGETS or type(value) is not dict or set(value) != FIELDS
            or type(value['schema']) is not int or value['schema'] != 1 or value['target'] != target
            or value['outcome'] not in ('running', 'success', 'failure') or type(value['failure_pending']) is not bool):
        raise ValueError(ERROR)
    _timestamp(value['started_ms'])
    for key in ('finished_ms', 'last_success_ms'):
        if value[key] is not None:
            _timestamp(value[key])
    if value['outcome'] == 'running':
        if value['finished_ms'] is not None:
            raise ValueError(ERROR)
    elif value['finished_ms'] is None or value['finished_ms'] < value['started_ms']:
        raise ValueError(ERROR)
    if value['outcome'] == 'success':
        if value['last_success_ms'] != value['finished_ms'] or value['failure_pending']:
            raise ValueError(ERROR)
    else:
        if value['last_success_ms'] is not None and value['last_success_ms'] > value['started_ms']:
            raise ValueError(ERROR)
        if value['outcome'] == 'failure' and not value['failure_pending']:
            raise ValueError(ERROR)
    return value


def parse_journal(raw, target):
    return validate_journal(_json(raw, JOURNAL_LIMIT), target)


def start_attempt(target, prior, now_ms):
    _timestamp(now_ms)
    if target not in TARGETS:
        raise ValueError(ERROR)
    previous_success, pending = None, False
    if prior is not None:
        validate_journal(prior, target)
        latest = max(value for value in (prior['started_ms'], prior['finished_ms'], prior['last_success_ms']) if value is not None)
        if now_ms < latest:
            raise ValueError(ERROR)
        previous_success = prior['last_success_ms']
        pending = prior['failure_pending'] or prior['outcome'] == 'running'
    return {'schema': 1, 'target': target, 'started_ms': now_ms, 'finished_ms': None,
            'outcome': 'running', 'last_success_ms': previous_success, 'failure_pending': pending}


def finish_attempt(running, success, now_ms):
    if type(running) is not dict:
        raise ValueError(ERROR)
    validate_journal(running, running.get('target'))
    _timestamp(now_ms)
    if type(success) is not bool or running['outcome'] != 'running' or now_ms < running['started_ms']:
        raise ValueError(ERROR)
    return {**running, 'finished_ms': now_ms, 'outcome': 'success' if success else 'failure',
            'last_success_ms': now_ms if success else running['last_success_ms'], 'failure_pending': not success}
