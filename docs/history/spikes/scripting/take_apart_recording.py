"""Takes a recording made by protocol 14 apart: puts the welcome back as protocol 13 wrote it (version 13, no nickname).

usage: take_apart.py FILE [fnv|crc]   prints the length of the old form and its hash (crc32, or fnv-1a 64).
The replay file: 8 bytes of magic, a u32 format version, then records of (varint length, u32 crc32 of the frame, the frame).
The welcome is the frame whose first byte is 1; its layout: kind, u32 version, identifier (varint length + bytes), u32 client,
then (new) the nickname as a string (varint length + bytes).
"""
import struct
import sys
import zlib


def read_varint(data, at):
    value = shift = 0
    while True:
        byte = data[at]
        at += 1
        value |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return value, at
        shift += 7


def write_varint(value):
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def old_form(frame, expected_version):
    assert frame[0] == 1
    version = struct.unpack_from('<I', frame, 1)[0]
    assert version == expected_version, version
    at = 5
    game_length, at = read_varint(frame, at)
    at += game_length
    at += 4  # the client
    nickname_length, after_length = read_varint(frame, at)
    nickname_end = after_length + nickname_length
    frame = frame[:at] + frame[nickname_end:]
    return frame[:1] + struct.pack('<I', expected_version - 1) + frame[5:]


def main():
    data = open(sys.argv[1], 'rb').read()
    kind = sys.argv[2] if len(sys.argv) > 2 else 'crc'
    out = bytearray(data[:12])
    at = 12
    changed = 0
    while at < len(data):
        length, after = read_varint(data, at)
        frame = data[after + 4:after + 4 + length]
        at = after + 4 + length
        if frame and frame[0] == 1:
            frame = old_form(frame, 14)
            changed += 1
        out += write_varint(len(frame)) + struct.pack('<I', zlib.crc32(frame)) + frame
    assert changed == 1, changed
    out = bytes(out)
    if kind == 'fnv':
        state = 0xcbf29ce484222325
        for byte in out:
            state ^= byte
            state = (state * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
        print(len(out), state)
    else:
        print(len(out), zlib.crc32(out))


main()
