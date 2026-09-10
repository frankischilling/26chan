"""One bounded request/response on a connected Unix byte stream."""
import os
import socket
import stat
import time

INPUT_BYTES = 8_388_608
OUTPUT_BYTES = 4_194_816
TRANSFER_SECONDS = 3
BUFFER_BYTES = 65_536


def remaining_timeout(connection, deadline):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError('dispatch transfer expired')
    connection.settimeout(remaining)


def read_exact(connection, size, deadline):
    # Only headers use this helper; payloads are streamed to a private file.
    result = bytearray()
    while len(result) < size:
        remaining_timeout(connection, deadline)
        data = connection.recv(size - len(result))
        if not data:
            raise ValueError('dispatch frame truncated')
        result.extend(data)
    return bytes(result)


def receive_request(connection, destination, *, deadline=None):
    deadline = time.monotonic() + TRANSFER_SECONDS if deadline is None else deadline
    header = read_exact(connection, 16, deadline)
    size = int.from_bytes(header[8:], 'big')
    if header[:8] != b'IBJOB001' or not 1 <= size <= INPUT_BYTES:
        raise ValueError('dispatch frame rejected')
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                         os.O_CLOEXEC | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'wb') as output:
        while size:
            remaining_timeout(connection, deadline)
            data = connection.recv(min(size, BUFFER_BYTES))
            if not data:
                raise ValueError('dispatch frame truncated')
            output.write(data)
            size -= len(data)
        remaining_timeout(connection, deadline)
        if connection.recv(1):
            raise ValueError('dispatch frame has trailing bytes')


def send_response(connection, source, *, deadline=None):
    deadline = time.monotonic() + TRANSFER_SECONDS if deadline is None else deadline
    descriptor = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as input_file:
        metadata = os.fstat(input_file.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size != OUTPUT_BYTES:
            raise ValueError('dispatch output rejected')
        remaining_timeout(connection, deadline)
        connection.sendall(b'IBOUT001' + OUTPUT_BYTES.to_bytes(8, 'big'))
        remaining = OUTPUT_BYTES
        while remaining:
            data = input_file.read(min(remaining, BUFFER_BYTES))
            if not data:
                raise ValueError('dispatch output truncated')
            remaining_timeout(connection, deadline)
            connection.sendall(data)
            remaining -= len(data)
        if input_file.read(1):
            raise ValueError('dispatch output has trailing bytes')
        remaining_timeout(connection, deadline)
        connection.shutdown(socket.SHUT_WR)
