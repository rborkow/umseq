// SPDX-License-Identifier: MIT
//! PROBE faithful structural port of replay/search.cpp; see ../LICENSE.star.
//! Whole-index validation is deliberately performed once by index::probe_load.
use umgpu::{ProbeConfig, ProbeOutput, ProbeRequest, ProbeStats};

#[derive(Clone, Copy)]
pub struct ProbeIndex<'a> {
    pub genome: &'a [u8],
    pub sa: &'a [u8],
    pub config: ProbeConfig,
}

#[inline(always)]
pub fn probe_packed_at(bytes: &[u8], position: u64, width: u64) -> u64 {
    let bit = position * width;
    let offset = (bit / 8) as usize;
    (u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) >> (bit % 8))
        & ((1u64 << width) - 1)
}

#[derive(Clone, Copy, Default)]
struct Point {
    position: u64,
    length: u64,
}
#[derive(Clone, Copy)]
struct Endpoint {
    current: Point,
    previous: Point,
    older: Point,
}
impl Endpoint {
    fn new(p: Point) -> Self {
        Self {
            current: p,
            previous: p,
            older: p,
        }
    }
    fn shift(&mut self, next: Point) {
        if next.length > self.current.length {
            self.older = self.previous;
            self.previous = self.current;
        }
        self.current = next;
    }
}
fn midpoint(a: u64, b: u64) -> u64 {
    a / 2 + b / 2 + (a % 2 + b % 2) / 2
}
struct Search<'a> {
    ix: ProbeIndex<'a>,
    reads: &'a [u8],
    r: ProbeRequest,
    stats: ProbeStats,
}
impl Search<'_> {
    #[inline]
    fn compare(&mut self, position: u64, length: u64, prefix: u64) -> Result<(u64, bool), u64> {
        let c = self.ix.config;
        let encoded = probe_packed_at(self.ix.sa, position, c.strand_bit + 1);
        self.stats.gathers += 1;
        self.stats.comparisons += 1;
        let forward = encoded >> c.strand_bit == 0;
        let address = encoded & !(1u64 << c.strand_bit);
        if address >= c.n_genome || prefix > length || length > self.r.length {
            return Err(3);
        }
        self.stats.directions |= 1 << ((1 - self.r.dir) * 2 + u64::from(!forward));
        // Preserve STAR's SA gather, but form no next-byte index for an empty comparison.
        if prefix == length {
            return Ok((length, false));
        }
        let base = if forward {
            address
        } else {
            c.n_genome - 1 - address
        };
        if (forward && address + length > c.n_genome + 200)
            || (!forward && (prefix > base || length > base + 201))
        {
            return Err(3);
        }
        let offset = if (self.r.dir == 1) == forward {
            self.r.s0
        } else {
            self.r.s1
        };
        let mut examined = 0;
        for k in prefix..length {
            let rp = if self.r.dir == 1 {
                self.r.start + k
            } else {
                self.r.start - k
            };
            let gp = if forward {
                200 + base + k
            } else {
                (200 + base) - k
            };
            let q = self.reads[(offset + rp) as usize];
            let g = self.ix.genome[gp as usize];
            examined += 1;
            self.stats.bytes += 1;
            if q != g {
                self.stats.max_compare = self.stats.max_compare.max(examined);
                return Ok((k, if forward { q > g } else { !(q > g || g > 3) }));
            }
        }
        self.stats.max_compare = self.stats.max_compare.max(examined);
        Ok((length, false)) // exact: ordering is never consumed
    }
    fn expand(&mut self, best: Point, side: Endpoint) -> Result<u64, u64> {
        let mut inside = side.previous.position;
        let mut outside = side.older;
        if side.current.length < best.length {
            inside = best.position;
            outside = side.current;
        } else if side.previous.length < side.current.length {
            inside = side.current.position;
            outside = side.previous;
        }
        while inside.abs_diff(outside.position) > 1 {
            self.stats.loops += 1;
            let middle = midpoint(inside, outside.position);
            let matched = self.compare(middle, best.length, outside.length)?.0;
            if matched == best.length {
                inside = middle;
            } else {
                outside = Point {
                    position: middle,
                    length: matched,
                };
            }
        }
        Ok(inside)
    }
    fn run(&mut self) -> Result<ProbeOutput, u64> {
        let r = self.r;
        if r.tag != 0 || r.dir > 1 {
            return Err(1);
        }
        if r.read_len == 0
            || r.read_len > 4096
            || r.length == 0
            || r.prefix > r.length
            || r.start >= r.read_len
            || r.low > r.high
            || r.high >= self.ix.config.n_sa
            || r.s0 > self.reads.len() as u64
            || r.read_len > self.reads.len() as u64 - r.s0
            || r.s1 > self.reads.len() as u64
            || r.read_len > self.reads.len() as u64 - r.s1
            || (r.dir == 1 && r.length > r.read_len - r.start)
            || (r.dir == 0 && r.length > r.start + 1)
        {
            return Err(2);
        }
        let first = Point {
            position: r.low,
            length: self.compare(r.low, r.length, r.prefix)?.0,
        };
        let last = Point {
            position: r.high,
            length: self.compare(r.high, r.length, r.prefix)?.0,
        };
        let mut lower = Endpoint::new(first);
        let mut upper = Endpoint::new(last);
        let mut best = first;
        let mut prefix = first.length.min(last.length);
        while upper.current.position - lower.current.position > 1 {
            self.stats.loops += 1;
            let middle = midpoint(lower.current.position, upper.current.position);
            let (matched, move_lower) = self.compare(middle, r.length, prefix)?;
            best = Point {
                position: middle,
                length: matched,
            };
            if best.length == r.length {
                break;
            }
            if move_lower {
                lower.shift(best);
            } else {
                upper.shift(best);
            }
            prefix = lower.current.length.min(upper.current.length);
        }
        if best.length < r.length {
            best = if lower.current.length > upper.current.length {
                lower.current
            } else {
                upper.current
            };
        }
        let low = self.expand(best, lower)?;
        let high = self.expand(best, upper)?;
        if low > high || low < r.low || high > r.high {
            return Err(4);
        }
        Ok(ProbeOutput {
            length: best.length,
            low,
            high,
            count: high - low + 1,
            status: 0,
        })
    }
}

pub fn probe_search(
    ix: ProbeIndex<'_>,
    reads: &[u8],
    r: ProbeRequest,
) -> (ProbeOutput, ProbeStats) {
    let mut s = Search {
        ix,
        reads,
        r,
        stats: ProbeStats::default(),
    };
    let out = s.run().unwrap_or_else(|status| ProbeOutput {
        status,
        ..Default::default()
    });
    (out, s.stats)
}
