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
