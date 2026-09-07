#!/usr/bin/env python3
"""Count the two distinct reverse predicates without altering captured requests."""
import argparse
import hashlib
import json
from pathlib import Path
import struct


def audit(path):
    data = path.read_bytes()
    if hashlib.sha256(data[:-120]).digest() != data[-32:]:
        raise ValueError(str(path) + ': footer hash mismatch')
    at = struct.unpack_from('<I', data, 12)[0]
    result = dict(path=str(path), sha256=hashlib.sha256(data).hexdigest(),
                  inner=0, reads=0, reverse_span=0, reverse_prefix=0, fully_known_boundary=0, examples=[])
    while at < len(data)-120:
        size, kind = struct.unpack_from('<IH', data, at)
        if kind == 1:
            v = struct.unpack_from('<16Q', data, at+16)
            direction = data[at+144]
            result['inner'] += 1
            if direction == 0:
                bad_span = v[8] > v[7]+1
                bad_prefix = v[11] > v[7]
                result['reverse_span'] += int(bad_span)
                result['reverse_prefix'] += int(bad_prefix)
                result['fully_known_boundary'] += int(bad_prefix and v[8] == v[11] == v[7]+1)
                if (bad_span or bad_prefix) and len(result['examples']) < 2:
                    result['examples'].append(dict(request=v[0], read=v[1], S=v[7], N=v[8], L_in=v[11], STAR=list(v[12:])))
        elif kind == 2:
            result['reads'] += 1
        else:
            raise ValueError('unexpected record type')
        if size < 16:
            raise ValueError('invalid frame')
        at += size
    if at != len(data)-120:
        raise ValueError('invalid frame extent')
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('root', type=Path)
    p.add_argument('output', type=Path)
    a = p.parse_args()
    files = sorted((a.root/'star').glob('*.ssir'))
    with a.output.open('x') as out:
        for file in files:
            out.write(json.dumps(audit(file))+'\n')
            out.flush()
    rows = [json.loads(line) for line in a.output.read_text().splitlines()]
    counts = {name:sum(row[name] for row in rows) for name in ('inner','reads','reverse_span','reverse_prefix','fully_known_boundary')}
    declared = json.loads((a.root/'star/capture-manifest.json').read_text())
    assert len(rows) == len(declared['workers']) == 20
    assert counts['inner'] == declared['actual_count']
    assert counts['reads'] == declared['complete_reads']
    print(json.dumps(dict(files=len(rows), **counts), indent=2))
