#!/usr/bin/env python3
"""Capture first-frame GIF raster behavior with synthetic bytes and libgd 2.3.3."""
import argparse
import ctypes
import ctypes.util
import json
from pathlib import Path
import struct


def gif(canvas, rectangle, transparent):
    left, top, width, height = rectangle
    codes = []
    for pixel in range(width * height):
        codes.extend([4, 1 if pixel % 3 else 0])
    codes.append(5)
    packed = sum(code << (3 * index) for index, code in enumerate(codes)).to_bytes(
        (len(codes) * 3 + 7) // 8, 'little')
    result = b'GIF89a' + struct.pack('<HHBBB', *canvas, 0x80, 0, 0)
    result += bytes([0, 0, 255, 255, 0, 0])
    if transparent:
        result += b'!\xf9\x04\x01\x00\x00\x01\x00'
    result += b',' + struct.pack('<HHHHB', *rectangle, 0)
    return result + b'\x02' + bytes([len(packed)]) + packed + b'\x00;'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    lib = ctypes.CDLL(ctypes.util.find_library('gd'))
    pointer, integer = ctypes.c_void_p, ctypes.c_int

    def bind(name, result, *arguments):
        function = getattr(lib, name)
        function.restype, function.argtypes = result, list(arguments)
        return function

    version = bind('gdVersionString', ctypes.c_char_p)
    if version() != b'2.3.3':
        raise SystemExit('The GIF reference requires libgd 2.3.3')
    decode = bind('gdImageCreateFromGifPtr', pointer, integer, pointer)
    create = bind('gdImageCreateTrueColor', pointer, integer, integer)
    pixel = bind('gdImageGetTrueColorPixel', integer, pointer, integer, integer)
    put = bind('gdImageSetPixel', None, pointer, integer, integer, integer)
    resize = bind('gdImageCopyResampled', None, pointer, pointer, *([integer] * 8))
    destroy = bind('gdImageDestroy', None, pointer)
    clip = bind('gdImageGetClip', None, pointer, *([ctypes.POINTER(integer)] * 4))
    cases = []
    for rectangle in [(0, 0, 4, 3), (1, 1, 2, 2), (3, 2, 1, 1)]:
        for transparent in [False, True]:
            for output in [(4, 3), (2, 1)]:
                data = gif((4, 3), rectangle, transparent)
                buffer = ctypes.create_string_buffer(data)
                source, destination = decode(len(data), buffer), create(*output)
                assert source and destination
                try:
                    bounds = [integer() for _ in range(4)]
                    clip(source, *[ctypes.byref(value) for value in bounds])
                    background = [255, 255, 238]
                    for y in range(output[1]):
                        for x in range(output[0]):
                            put(destination, x, y, 0xffffee)
                    # make_thumb passes GetImageSize's logical-screen dimensions,
                    # even when GD's decoded first-frame raster is smaller.
                    resize(destination, source, 0, 0, 0, 0, *output, 4, 3)
                    expected = []
                    for y in range(output[1]):
                        for x in range(output[0]):
                            value = pixel(destination, x, y)
                            expected.extend([(value >> 16) & 255, (value >> 8) & 255, value & 255])
                    cases.append(dict(input_hex=data.hex(), canvas=[4, 3], rectangle=list(rectangle),
                                      transparent=transparent, output=list(output), background=background,
                                      gd_clip=[value.value for value in bounds], expected=expected))
                finally:
                    destroy(source)
                    destroy(destination)
    result = dict(version='2.3.3', reference='https://github.com/libgd/libgd/tree/gd-2.3.3',
                  scope='Synthetic first-frame GIF pixels before JPEG encoding; deployed source GD version unknown',
                  cases=cases)
    path = Path(__file__).resolve().parents[1] / 'fixtures/gd-gif-thumbnails.json'
    if args.check:
        assert json.loads(path.read_text()) == result
        print(f'GD 2.3.3: {len(cases)} first-frame GIF cases match')
    else:
        print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
