"""Independent Pillow observations for tiny, hand-authored GIF animations."""
import io
import struct
from PIL import Image

RED = (255, 0, 0, 255)
BLUE = (0, 0, 255, 255)
GREEN = (0, 255, 0, 255)
EXPECTED = [
    [RED, RED, RED, RED],
    [BLUE, RED, RED, RED],
    [RED, RED, RED, GREEN],
    [RED, BLUE, RED, RED],
]
DELAYS = [70, 130, 170, 230]


def animation(loop):
    # Four global colors, red background, a 2x2 logical screen.
    data = bytearray(b'GIF89a' + struct.pack('<HHBBB', 2, 2, 0x81, 0, 0))
    data.extend(bytes([255, 0, 0, 0, 0, 255, 0, 255, 0, 0, 0, 0]))
    if loop is not None:
        data.extend(b'!\xff\x0bNETSCAPE2.0\x03\x01' + struct.pack('<H', loop) + b'\x00')
    for left, top, width, height, pixels, disposal, transparent, delay in [
        (0, 0, 2, 2, [0, 0, 0, 0], 1, False, 7),
        (0, 0, 1, 1, [1], 2, False, 13),
        (1, 1, 1, 1, [2], 3, False, 17),
        (0, 0, 2, 1, [0, 1], 1, True, 23),
    ]:
        data.extend(b'!\xf9\x04' + bytes([(disposal << 2) | int(transparent)]))
        data.extend(struct.pack('<HBB', delay, 0, 0))
        data.extend(b',' + struct.pack('<HHHHB', left, top, width, height, 0))
        # CLEAR before each pixel keeps every code at three bits.
        codes = [code for pixel in pixels for code in (4, pixel)] + [5]
        packed = sum(code << (index * 3) for index, code in enumerate(codes))
        compressed = packed.to_bytes((len(codes) * 3 + 7) // 8, 'little')
        data.extend(bytes([2, len(compressed)]) + compressed + b'\x00')
    return bytes(data + b';')


def observe(data):
    with Image.open(io.BytesIO(data)) as image:
        loop = image.info.get('loop')
        frames, delays = [], []
        for index in range(image.n_frames):
            image.seek(index)
            rgba = image.convert('RGBA')
            frames.append([rgba.getpixel((x, y)) for y in range(2) for x in range(2)])
            delays.append(image.info.get('duration'))
        return loop, frames, delays


if __name__ == '__main__':
    for repeat in (None, 0, 5):
        assert observe(animation(repeat)) == (repeat, EXPECTED, DELAYS)
    print('3 independent GIF timing, disposal, transparency and loop observations passed')
