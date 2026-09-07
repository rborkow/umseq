#!/usr/bin/env python3
"""Convert complete, independently SSIRv1-validated INNER batches to UMPROBE1.

This is intentionally a bounded assembly tool, not an SSIR parser replacement:
the caller must supply a validator built from the unchanged SSIR `format.cpp`.
It accepts complete files only, keeps whole reads, and stops before the declared
upper bound rather than inventing a cross-read exact-million boundary.
"""
import argparse, hashlib, json, pathlib, struct, subprocess, sys

MAGIC=b"UMPROBE1"; STAR=b"UMSTAR01"; CAP=1_000_000
def u64(b, p): return struct.unpack_from("<Q", b, p)[0]
def die(s): raise ValueError(s)

def checked(path, validator, source, index, runtime):
    subprocess.run([validator, str(path), source, index, runtime], check=True)
    b=path.read_bytes()
    if b[:8] != b"SSIRLE01" or len(b)<280: die("not SSIR v1")
    h=struct.unpack_from("<I", b, 12)[0]
    if h < 160 or h > 1024*1024: die("invalid SSIR header length")
    return b, h

def records(b, at):
    end=len(b)-120; current=[]
    while at<end:
        length, kind, flags, seq=struct.unpack_from("<IHHQ", b, at)
        if flags or length<16 or at+length>end: die("invalid validated record framing")
        payload=at+16
        if kind==1:
            if length<240: die("short INNER")
            v=struct.unpack_from("<16Q",b,payload)
            direction=b[payload+128]
            b0,b1=struct.unpack_from("<II",b,payload+132)
            if b0 != b1 or 240+b0+b1 != length: die("INNER buffer extent")
            buffers=payload+224
            current.append((v,direction,b[buffers:buffers+b0],b[buffers+b0:buffers+b0+b1]))
        elif kind==2:
            yield current; current=[]
        else: die("unexpected record kind")
        at += length
    if current: die("partial read at footer")

def main():
 p=argparse.ArgumentParser(); p.add_argument('--validator',required=True); p.add_argument('--source-sha256',required=True); p.add_argument('--index-sha256',required=True); p.add_argument('--runtime-sha256',required=True); p.add_argument('--index-parameters-sha256',required=True); p.add_argument('--output',type=pathlib.Path,required=True); p.add_argument('--manifest',type=pathlib.Path,required=True); p.add_argument('--upper-bound',type=int,default=CAP); p.add_argument('inputs',nargs='+'); a=p.parse_args()
 if not 1<=a.upper_bound<=CAP: die('upper bound must be 1..1000000')
 if a.output.exists() or a.manifest.exists(): die('refusing to overwrite')
 selected=[]; source_files=[]; stopped=False
 for name in a.inputs:
  path=pathlib.Path(name); b,h=checked(path,a.validator,a.source_sha256,a.index_sha256,a.runtime_sha256)
  for read in records(b,h):
   if len(selected)+len(read)>a.upper_bound:
    stopped=True; break
   selected.extend(read)
  source_files.append({'path':str(path),'sha256':hashlib.sha256(b).hexdigest()})
  if stopped: break
 if not selected: die('no complete-read INNER requests selected')
 arena=bytearray(); requests=[]; tuples=[]
 for v,d,b0,b1 in selected:
  # SSIR INNER: S,N,L_in,i1,i2,dir plus actual L_out/lo/hi/Nrep.
  s,n,lin,i1,i2=v[7],v[8],v[11],v[9],v[10]
  # `L_in` is the prefix already known by STAR.  Zero is a real value: an N
  # mark or bad upper bound can legitimately leave no known prefix.  Do not
  # turn those calls into a synthetic positive-prefix workload.
  if not (n and 0 <= lin <= n and i1 <= i2 and s < len(b0)):
   die('INNER is not a bounded real request')
  o0=len(arena); arena.extend(b0); o1=len(arena); arena.extend(b1)
  requests.append((0,o0,o1,len(b0),s,n,lin,i1,i2,d))
  tuples.append((v[12],v[13],v[14],v[15]))
 with a.output.open('xb') as f:
  f.write(MAGIC); f.write(struct.pack('<QQQ',len(requests),len(arena),0))
  for x in requests: f.write(struct.pack('<10Q',*x))
  f.write(arena)
 side=a.output.with_suffix('.star-tuples.bin')
 with side.open('xb') as f:
  f.write(STAR); f.write(struct.pack('<Q',len(tuples)))
  for x in tuples: f.write(struct.pack('<4Q',*x))
 digest=hashlib.sha256(a.output.read_bytes()).hexdigest()
 m={'source_kind':'STAR_INNER_CAPTURE','ssir_wire':'SSIRv1 unchanged; format.cpp validator required','selection_rule':'input order, complete read boundaries only; no per-thread global-first-N claim','hard_upper_bound':a.upper_bound,'actual_count':len(requests),'stopped_before_next_complete_read':stopped,'source_files':source_files,'request_sha256':digest,'star_tuple_sha256':hashlib.sha256(side.read_bytes()).hexdigest(),'source_sha256':a.source_sha256,'index_sha256':a.index_sha256,'runtime_sha256':a.runtime_sha256,'index_parameters_sha256':a.index_parameters_sha256}
 a.output.with_suffix('.probe-provenance.txt').write_text('STAR inner capture; exact observed buffers and outputs\n'+'\n'.join(f'{k}={json.dumps(v,sort_keys=True) if isinstance(v,(dict,list,bool)) else v}' for k,v in m.items())+'\n')
 a.manifest.write_text(json.dumps(m,indent=2,sort_keys=True)+'\n')
if __name__=='__main__':
 try: main()
 except Exception as e: print(f'ssir_to_umprobe: {e}',file=sys.stderr); sys.exit(2)
