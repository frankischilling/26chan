"""Deterministic qualification bytes; no decoder or privileged operation."""
import pathlib
import os
import sys
import struct
import zlib

REPO = pathlib.Path(__file__).resolve().parents[2]
COMPONENT_BYTES = 8_388_608
RESULT_BYTES = 4_456_960
BINDING = bytes(range(32))


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def png(width=1, height=1, size=None):
    data = (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress((b'\0' + b'\xff\0\0\xff' * width) * height)))
    if size is not None:
        padding = size - len(data) - 24
        if padding < 0:
            raise ValueError('PNG target too small')
        data += chunk(b'vpAg', bytes(padding))
    return data + chunk(b'IEND', b'')


def replay(name='empty'):
    return (REPO / 'apps/media-guest/tests/fixtures/replay' / f'{name}.tgkr').read_bytes()


def wire(name='empty'):
    return (REPO / 'tests/media/fixtures/replay-wire' / f'{name}.ibr').read_bytes()


def maximum_replay():
    """Same recorder body, exact input cap, legal empty DEFLATE blocks."""
    original = replay()
    body = zlib.decompress(original[12:], -15)
    for prefix_length in range(min(32, len(body))):
        prefix = body[:prefix_length]
        stored = b'\x00' + struct.pack('<HH', len(prefix), 65535 - len(prefix)) + prefix
        compressor = zlib.compressobj(6, zlib.DEFLATED, -15)
        compressed = compressor.compress(body[prefix_length:]) + compressor.flush()
        padding = COMPONENT_BYTES - 12 - len(stored) - len(compressed)
        if padding >= 0 and padding % 5 == 0:
            return original[:12] + b'\x00\x00\x00\xff\xff' * (padding // 5) + stored + compressed
    raise ValueError('cannot construct exact-cap replay')


def frame(image=None, replay_bytes=None):
    image = png() if image is None else image
    return (b'IBPAIR02' + struct.pack('>HHIQQ', 2, 48, int(replay_bytes is not None), len(image),
                                    len(replay_bytes) if replay_bytes is not None else 0)
            + bytes.fromhex('33' * 16) + image + (replay_bytes or b'') + b'IBDONE02')


def request(body=None, binding=BINDING):
    body = frame() if body is None else body
    if len(binding) != 32:
        raise ValueError('binding must have exactly 32 bytes')
    return b'IBJOB002' + struct.pack('>Q', len(body)) + binding + body


def check_result(data, *, binding=BINDING, width=1, height=1, replay_name=None):
    """Exact frozen wire comparison, never replay admission or publication."""
    expected_wire = b'' if replay_name is None else wire(replay_name)
    pixels = b'IBRGBA01' + struct.pack('>II', width, height) + b'\xff\0\0\xff' * width * height
    header = (b'IBRES002' + struct.pack('>HHI', 2, 64, int(replay_name is not None)) + binding
              + struct.pack('>QQ', len(pixels), len(expected_wire)))
    used = header + pixels + expected_wire
    if len(data) != RESULT_BYTES or data[:len(used)] != used or any(data[len(used):]):
        raise ValueError('paired candidate bytes differ from independent expectation')


def mutate_disk(data, mutation):
    """Only test disk bytes change; no runner or guest policy changes."""
    data = bytearray(data)
    if mutation == 'padding':
        data[-1] = 1
    elif mutation == 'extra-sector':
        data.extend(bytes(512))
    elif mutation == 'short-sector':
        # Preserve block-device-compatible length while truncating content.
        if len(data) <= 512:
            raise ValueError('truncation control needs multiple sectors')
        data = data[:512]
    elif mutation == 'outer-version':
        data[7] = ord('1')
    elif mutation == 'declared-length':
        data[8:16] = (16_777_273).to_bytes(8, 'big')
    else:
        raise ValueError('unknown disk mutation')
    return bytes(data)


def qualification_guard(*, root=True):
    """Fail closed before qualification activity, including direct API calls."""
    if sys.flags.optimize:
        raise RuntimeError('optimized Python cannot run qualification')
    if os.environ.get('APP_ENV') != 'development':
        raise RuntimeError('explicit APP_ENV=development is required')
    if os.environ.get('MEDIA_PAIRED_VM_QUALIFY') != '1':
        raise RuntimeError('explicit MEDIA_PAIRED_VM_QUALIFY=1 is required')
    real, effective = os.getuid(), os.geteuid()
    if real != effective or (root and effective != 0) or (not root and effective == 0):
        raise RuntimeError('qualification identity rejected')
