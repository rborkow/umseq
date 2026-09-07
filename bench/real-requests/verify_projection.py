#!/usr/bin/env python3
"""Independently check every converted field/buffer/STAR tuple against raw SSIR."""
import hashlib
import json
from pathlib import Path
import struct
import sys


def verify(capture, result):
    manifest = json.loads((result/'real-requests.json').read_text())
    wire = (result/'real-requests.bin').read_bytes()
    oracle = (result/'real-requests.star-tuples.bin').read_bytes()
    assert wire[:8] == b'UMPROBE1' and oracle[:8] == b'UMSTAR01'
    count, arena_size, reserved = struct.unpack_from('<3Q',wire,8)
    assert reserved == 0 and count == manifest['actual_count']
    assert len(wire) == 32 + count*80 + arena_size
    assert len(oracle) == 16 + count*32 and struct.unpack_from('<Q',oracle,8)[0] == count
    assert hashlib.sha256(wire).hexdigest() == manifest['request_sha256']
    assert hashlib.sha256(oracle).hexdigest() == manifest['star_tuple_sha256']
    arena = memoryview(wire)[32+count*80:]
    index = 0
    for entry in manifest['source_files']:
        source = capture/'star'/Path(entry['path']).name
        raw = source.read_bytes()
        assert hashlib.sha256(raw).hexdigest() == entry['sha256']
        assert hashlib.sha256(raw[:-120]).digest() == raw[-32:]
        at = struct.unpack_from('<I',raw,12)[0]
        while at < len(raw)-120:
            size, kind = struct.unpack_from('<IH',raw,at)
            if kind == 1:
                values = struct.unpack_from('<16Q',raw,at+16)
                direction = raw[at+144]
                n0,n1 = struct.unpack_from('<2I',raw,at+148)
                assert n0 == n1 and size == 240+n0+n1
                request = struct.unpack_from('<10Q',wire,32+index*80)
                assert request[0] == 0
                assert request[3:] == (n0,values[7],values[8],values[11],values[9],values[10],direction), index
                assert arena[request[1]:request[1]+n0] == raw[at+240:at+240+n0], index
                assert arena[request[2]:request[2]+n1] == raw[at+240+n0:at+240+n0+n1], index
                assert struct.unpack_from('<4Q',oracle,16+index*32) == values[12:16], index
                index += 1
            else:
                assert kind == 2
            at += size
        assert at == len(raw)-120
    declared = json.loads((capture/'star/capture-manifest.json').read_text())
    assert index == count == declared['actual_count']
    assert len(manifest['source_files']) == len(declared['workers']) == 20
    return dict(source_files=20, requests=index, request_fields_and_buffers_matched=index,
                captured_star_tuples_matched=index, skipped=0,
                request_sha256=manifest['request_sha256'], star_tuple_sha256=manifest['star_tuple_sha256'])


if __name__ == '__main__':
    print(json.dumps(verify(Path(sys.argv[1]),Path(sys.argv[2])),indent=2))
