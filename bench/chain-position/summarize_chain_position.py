#!/usr/bin/env python3
"""Summarize the bounded chain-position sidecar and reconcile SPLIT counters."""
import argparse
import json
from pathlib import Path
import sys

def share(n, d):
    return None if d == 0 else n / d

def add(a, b):
    return {k: a.get(k, 0) + b.get(k, 0) for k in ('inner_requests', 'gathers', 'compared_bytes')}

def main(chain_path, split_path=None):
    chain = json.loads(Path(chain_path).read_text())
    if chain.get('schema') != 'chain-position-v1': raise ValueError('unexpected chain schema')
    zero = {'inner_requests': 0, 'gathers': 0, 'compared_bytes': 0}
    actual = {k: chain['actual'][k] for k in zero}
    if chain['actual'].get('complete_tuple_checks', actual['inner_requests']) != actual['inner_requests']:
        raise ValueError('observed tuple checks do not cover every inner request')
    initial, grid_only, tail = chain['work_partition']
    union = add(initial, grid_only)
    offset_total = zero.copy()
    for row in chain['offsets']:
        r = {k: row[k] for k in zero}
        offset_total = add(offset_total, r)
    if offset_total != actual: raise ValueError('offset work does not reconcile actual work')
    joint_total, joint_initial = zero.copy(), zero.copy()
    for row in chain['joint_gathers_bytes_by_initial_istart_iDir']:
        joint_total = add(joint_total, row)
        if row['initial']: joint_initial = add(joint_initial, row)
    if joint_total != actual or joint_initial != initial: raise ValueError('joint work does not reconcile initial/actual work')
    if add(union, tail) != actual: raise ValueError('union + adaptive tail does not reconcile actual work')
    if add(initial, grid_only) != union: raise ValueError('initial + grid-only overlap accounting failed')
    if split_path:
        split = json.loads(Path(split_path).read_text())
        inner = split['inner']
        if actual['gathers'] != sum(inner['calls_by_direction']): raise ValueError('gather count disagrees with split counters')
        if actual['compared_bytes'] != sum(inner['compared_bytes_by_direction']): raise ValueError('byte count disagrees with split counters')
        if actual['inner_requests'] != split['inner_requests']: raise ValueError('inner request count disagrees with split counters')
    report = {'status': 'OK', 'actual': actual, 'initial': initial, 'grid_only_added': grid_only,
              'union_initial_grid20': union, 'adaptive_tail': tail,
              'initial_gather_share': share(initial['gathers'], actual['gathers']),
              'initial_byte_share': share(initial['compared_bytes'], actual['compared_bytes']),
              'initial_grid20_gather_share': share(union['gathers'], actual['gathers']),
              'recommend_grid20': False if share(initial['gathers'], actual['gathers']) is None else share(initial['gathers'], actual['gathers']) < .5,
              'reverse_suppression': chain['reverse_suppression']}
    return report

if __name__ == '__main__':
    p=argparse.ArgumentParser();p.add_argument('--chain',type=Path,required=True);p.add_argument('--split',type=Path);p.add_argument('--output',type=Path)
    a=p.parse_args()
    try: out=main(a.chain,a.split)
    except Exception as e: print('chain-position summary failed: '+str(e),file=sys.stderr);sys.exit(1)
    text=json.dumps(out,indent=2,sort_keys=True)+'\n'
    if a.output: a.output.write_text(text)
    else: print(text,end='')
