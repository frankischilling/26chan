#!/usr/bin/env python3
"""Independently transcribe pinned recorder fixtures, never a production decoder.

Default: verify frozen wire fixtures. --record: deliberately replace them.
This uses Python's raw DEFLATE and struct routines, not either Rust codec.
Integrity and framing checks remain active under Python optimization.
"""

import hashlib
from pathlib import Path
import struct
import sys
import zlib


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SOURCE = ROOT / "apps/media-guest/tests/fixtures/replay"
PINNED = {
    "empty": "f43a6b843eb6078492f6aafa1dd1c95ed89e184622b6e65e75886ad22f6d2bb8",
    "commands": "f70ca7c89e105986da4d995bf70df141f55680b1467614445e8b9eefcf2e10b7",
}
PAYLOAD_SIZE = {
    0: 0, 1: 6, 2: 6, 3: 0, 4: 0, 5: 0, 6: 3, 7: 4, 8: 4,
    10: 1, 11: 1, 12: 4, 13: 1, 14: 1, 15: 1, 16: 1, 17: 1, 18: 4,
    20: 0, 21: 0, 22: 1, 23: 0, 24: 1, 25: 1, 26: 1, 27: 4,
    254: 0, 255: 0,
}
MAX_EVENTS = 16384
MAX_BODY_BYTES = 179 + MAX_EVENTS * 11


def require(condition, message):
    if not condition:
        raise ValueError(message)


def transcribe(source):
    require(len(source) >= 12, "truncated source header")
    require(source[:4] == b"TGK\x01" and source[8:12] == bytes([0, 9, 4, 1]),
            "source magic or version differs")
    body_bytes = struct.unpack_from(">I", source, 4)[0]
    require(189 <= body_bytes <= MAX_BODY_BYTES, "source body size out of bounds")
    inflater = zlib.decompressobj(wbits=-15)
    try:
        body = inflater.decompress(source[12:], body_bytes + 1)
    except zlib.error as error:
        raise ValueError("invalid source DEFLATE stream") from error
    require(inflater.eof and not inflater.unused_data and not inflater.unconsumed_tail,
            "source DEFLATE framing differs")
    require(len(body) == body_bytes, "source body length differs")
    require(body[:2] == b"\x00\x15" and body[21:23] == bytes([8, 19]),
            "source metadata or tool-table framing differs")
    count = struct.unpack_from(">I", body, 175)[0]
    require(2 <= count <= MAX_EVENTS, "source event count out of bounds")
    packet = bytearray(256 + count * 16)
    packet[:8] = b"IBRPLY01"
    struct.pack_into(">HHHHII", packet, 8, 1, 64, 1, 0, len(packet), count)
    packet[24:35] = body[10:21]  # Dimensions, colors, initial tool.
    packet[36:44] = body[2:10]  # Source start and end seconds.
    packet[44:48] = source[8:12]
    seen = set()
    for offset in range(23, 175, 19):
        tool = body[offset:offset + 19]
        tool_id = tool[0]
        require(1 <= tool_id <= 8 and tool_id not in seen, "source tool ID differs")
        seen.add(tool_id)
        toggles = [tool[10], tool[11], tool[12], tool[18]]
        require(all(value in [0, 1] for value in toggles), "source tool toggle differs")
        start = 64 + (tool_id - 1) * 24
        packet[start:start + 4] = bytes([
            tool_id, tool[1], sum(value << bit for bit, value in enumerate(toggles)), tool[13]
        ])
        packet[start + 4:start + 12] = tool[2:10]
        packet[start + 12:start + 16] = tool[14:18]
    offset = 179
    for index in range(count):
        require(offset + 5 <= len(body), "truncated source event header")
        tag = body[offset]
        require(tag in PAYLOAD_SIZE, "unknown source event tag")
        length = PAYLOAD_SIZE[tag]
        require(offset + 5 + length <= len(body), "truncated source event payload")
        require((index == 0) == (tag == 0) and (index == count - 1) == (tag == 255),
                "source endpoint marker differs")
        start = 256 + index * 16
        packet[start] = tag
        packet[start + 4:start + 8] = body[offset + 1:offset + 5]
        packet[start + 8:start + 8 + length] = body[offset + 5:offset + 5 + length]
        offset += 5 + length
    require(offset == len(body), "extra source event bytes")
    return bytes(packet)


def main():
    require(sys.argv[1:] in ([], ["--record"]), "usage: generate.py [--record]")
    # Check every input before record mode is allowed to replace any fixture.
    packets = {}
    for name, expected_hash in PINNED.items():
        source = (SOURCE / f"{name}.tgkr").read_bytes()
        require(hashlib.sha256(source).hexdigest() == expected_hash, "input pin mismatch: " + name)
        packets[name] = transcribe(source)
    for name, packet in packets.items():
        target = HERE / f"{name}.ibr"
        if sys.argv[1:] == ["--record"]:
            target.write_bytes(packet)
        else:
            require(target.read_bytes() == packet, "frozen output differs: " + name)
        print(f"{name}.ibr: {len(packet)} bytes; sha256 {hashlib.sha256(packet).hexdigest()}")


if __name__ == "__main__":
    main()
