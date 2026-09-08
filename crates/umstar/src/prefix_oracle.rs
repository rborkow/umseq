//! Independent CPU transcription of STAR's outer prefix decisions for validation.
//! Inner comparisons use the already gated Rust probe, not the C++ device body.
use umgpu::{ProbeConfigV2, ProbeOutput, ProbeOutputV2, ProbeRequestV2};
use umseed_probe::cpu::{ProbeIndex, probe_packed_at, probe_search};

pub fn prefix_oracle(
    genome: &[u8],
    sa: &[u8],
    sai: &[u8],
    reads: &[u8],
    c: ProbeConfigV2,
    request: ProbeRequestV2,
) -> ProbeOutputV2 {
    let mut r = request.inner;
    let reject = |status| ProbeOutputV2 {
        inner: ProbeOutput {
            status,
            ..Default::default()
        },
        branch: 0,
    };
    if r.tag == 0 {
        return ProbeOutputV2 {
            inner: probe_search(
                ProbeIndex {
                    genome,
                    sa,
                    config: c.inner,
                },
                reads,
                r,
            )
            .0,
            branch: 0,
        };
    }
    if r.tag != 1 || r.dir > 1 {
        return reject(1);
    }
    if c.sparse != 1 || c.seed_search_lmax != 0 || request.distance != 0 {
        return reject(5);
    }
    let mut lind = c.index_bases.min(r.length);
    let mut key = 0;
    for i in 0..lind {
        let b = reads[(r.s0 + if r.dir == 1 { r.start + i } else { r.start - i }) as usize];
        if b > 3 {
            return reject(8);
        }
        key = 4 * key + if r.dir == 1 { b as u64 } else { 3 - b as u64 };
    }
    let get = |i| probe_packed_at(&sai[c.sai_offset as usize..], i, c.sai_width);
    let mut first = 0;
    while lind > 0 {
        first = get(c.starts[lind as usize - 1] + key);
        if first & c.absent_mask == 0 {
            break;
        }
        lind -= 1;
        key >>= 2;
    }
    if lind == 0 {
        return reject(6);
    }
    let next = c.starts[lind as usize - 1] + key + 1;
    let mut good = true;
    let last = if next < c.starts[lind as usize] && get(next) & c.absent_mask == 0 {
        (get(next) & c.n_mask).wrapping_sub(1)
    } else {
        good = false;
        c.inner.n_sa - 1
    };
    let no_n = first & c.n_mask_c == 0;
    r.low = first & c.n_mask;
    r.high = last;
    if r.low > last || last >= c.inner.n_sa {
        return reject(7);
    }
    if lind < c.index_bases && no_n && good {
        return ProbeOutputV2 {
            inner: ProbeOutput {
                length: lind,
                low: first,
                high: last,
                count: last - first + 1,
                status: 0,
            },
            branch: 1,
        };
    }
    r.tag = 0;
    r.prefix = if no_n && good { lind } else { 0 };
    let unique = first == last && no_n && good;
    let output = probe_search(
        ProbeIndex {
            genome,
            sa,
            config: c.inner,
        },
        reads,
        r,
    )
    .0;
    ProbeOutputV2 {
        inner: output,
        branch: if unique { 2 } else { 3 },
    }
}

/// Independent transcription of mapOneRead's chain, using the per-step Rust oracle.
/// Input validation is transport policy; the while/Shift/flag expressions follow STAR.
pub fn chain_oracle(
    genome: &[u8],
    sa: &[u8],
    sai: &[u8],
    reads: &[u8],
    c: ProbeConfigV2,
    q: umgpu::ProbeRequestV3,
) -> umgpu::ProbeOutputV3 {
    use umgpu::{ProbeOutputV3, ProbeRequest, ProbeStepV3};
    let mut o = ProbeOutputV3::default();
    if q.dir > 1 {
        o.status = 1;
        return o;
    }
    if c.sparse != 1 || c.seed_search_lmax != 0 {
        o.status = 5;
        return o;
    }
    if q.read_len == 0
        || q.read_len > 4096
        || q.piece_start > q.read_len
        || q.piece_length > q.read_len - q.piece_start
        || q.nstart == 0
        || q.istart >= q.nstart
        || q.s0 > reads.len() as u64
        || q.read_len > reads.len() as u64 - q.s0
        || q.s1 > reads.len() as u64
        || q.read_len > reads.len() as u64 - q.s1
        || q.istart.checked_mul(q.lstart).is_none()
    {
        o.status = 2;
        return o;
    }
    let mut lmapped = 0;
    // u128 protects malformed transport sums; admitted STAR coordinates fit u64.
    while u128::from(q.istart) * u128::from(q.lstart)
        + u128::from(lmapped)
        + u128::from(q.seed_map_min)
        < u128::from(q.piece_length)
    {
        if o.n_steps == 8 {
            o.status = 9;
            break;
        }
        if o.n_steps == q.max_steps {
            o.status = 10;
            break;
        }
        let shift = if q.dir == 1 {
            q.piece_start + q.istart * q.lstart + lmapped
        } else {
            q.piece_start + q.piece_length - q.istart * q.lstart - 1 - lmapped
        };
        let length = q.piece_length - lmapped - q.istart * q.lstart;
        let result = prefix_oracle(
            genome,
            sa,
            sai,
            reads,
            c,
            ProbeRequestV2 {
                inner: ProbeRequest {
                    tag: 1,
                    s0: q.s0,
                    s1: q.s1,
                    read_len: q.read_len,
                    start: shift,
                    length,
                    dir: q.dir,
                    ..Default::default()
                },
                distance: 0,
            },
        );
        let r = result.inner;
        o.steps[o.n_steps as usize] = ProbeStepV3 {
            shift,
            max_l: r.length,
            nrep: r.count,
            low: r.low,
            high: r.high,
            branch: result.branch,
            status: r.status,
        };
        o.n_steps += 1;
        if r.status != 0 {
            o.status = r.status;
            break;
        }
        if r.length > length {
            o.status = 4;
            break;
        }
        if q.dir == 1 && q.istart == 0 && lmapped == 0 && shift + r.length == q.piece_length {
            o.flag_dir_map_cleared = 1;
        }
        if r.length == 0 {
            o.status = 11;
            break;
        }
        lmapped += r.length;
    }
    o
}
