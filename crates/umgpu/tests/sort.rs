#![cfg(feature = "cuda")]

use std::time::Instant;

use umem::{Allocation, AnyBuf, Buf, Rw};
use umgpu::{
    Context, ContextOptions, inc_u64, radix_sort_pairs_u64_u32, radix_sort_pairs_u64_u32_temp_size,
    submit,
};

const N: usize = 16 * 1024 * 1024;

fn allocation() -> Allocation {
    Allocation::Anon {
        huge: true,
        require_huge: false,
    }
}
fn rw(buf: AnyBuf) -> Buf<Rw> {
    match buf {
        AnyBuf::Rw(buf) => buf,
        AnyBuf::Ro(_) => panic!("expected writable buffer"),
    }
}

#[test]
#[ignore = "requires a CUDA GPU with HMM/ATS host-memory access"]
fn radix_sort_and_hmm_access_path() {
    for host_register in [false, true] {
        let ctx = Context::new(0, ContextOptions { host_register }).expect("CUDA context");
        println!(
            "host_register={host_register}, props={:?}",
            ctx.device_props()
        );
        let mut keys = Buf::<Rw>::allocate(N * 8, allocation()).unwrap();
        let mut vals = Buf::<Rw>::allocate(N * 4, allocation()).unwrap();
        let keys_out = Buf::<Rw>::allocate(N * 8, allocation()).unwrap();
        let vals_out = Buf::<Rw>::allocate(N * 4, allocation()).unwrap();
        let temp =
            Buf::<Rw>::allocate(radix_sort_pairs_u64_u32_temp_size(N).unwrap(), allocation())
                .unwrap();
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        for (i, key) in keys.as_pod_mut_slice::<u64>().iter_mut().enumerate() {
            state ^= state << 7;
            state ^= state >> 9;
            *key = state;
            vals.as_pod_mut_slice::<u32>()[i] = i as u32;
        }
        let original = keys.as_pod_slice::<u64>().to_vec();
        let uctx = ctx.umem_context();
        let keys = keys.lease(&uctx);
        let vals = vals.lease(&uctx);
        let keys_out = keys_out.lease(&uctx);
        let vals_out = vals_out.lease(&uctx);
        let temp = temp.lease(&uctx);
        radix_sort_pairs_u64_u32(
            &ctx,
            ctx.default_stream(),
            &keys,
            &vals,
            &keys_out,
            &vals_out,
            &temp,
            N,
            0..64,
        )
        .unwrap();
        let buffers = submit(
            &ctx,
            ctx.default_stream(),
            vec![
                keys.erase(),
                vals.erase(),
                keys_out.erase(),
                vals_out.erase(),
                temp.erase(),
            ],
        )
        .unwrap()
        .wait()
        .unwrap();
        let mut buffers = buffers.into_iter();
        let _keys_in = rw(buffers.next().unwrap());
        let _vals_in = rw(buffers.next().unwrap());
        let sorted = rw(buffers.next().unwrap());
        let sorted_vals = rw(buffers.next().unwrap());
        let _temp = rw(buffers.next().unwrap());
        let sorted_keys = sorted.as_pod_slice::<u64>();
        assert!(sorted_keys.windows(2).all(|w| w[0] <= w[1]));
        let permutation = sorted_vals.as_pod_slice::<u32>();
        let mut seen = vec![false; N];
        for &index in permutation {
            let index = index as usize;
            assert!(index < N);
            assert!(!seen[index], "duplicate output index {index}");
            seen[index] = true;
        }
        assert!(seen.into_iter().all(|present| present));
        assert!(
            sorted_keys
                .iter()
                .zip(permutation)
                .all(|(&key, &i)| key == original[i as usize])
        );

        let access_bytes = 1024 * 1024 * 1024_usize;
        let mut input = Buf::<Rw>::allocate(access_bytes, allocation()).unwrap();
        input.as_pod_mut_slice::<u64>().fill(41);
        let output = Buf::<Rw>::allocate(access_bytes, allocation()).unwrap();
        let input = input.lease(&uctx);
        let output = output.lease(&uctx);
        let start = Instant::now();
        inc_u64(
            &ctx,
            ctx.default_stream(),
            &input,
            &output,
            access_bytes / 8,
        )
        .unwrap();
        let buffers = submit(
            &ctx,
            ctx.default_stream(),
            vec![input.erase(), output.erase()],
        )
        .unwrap()
        .wait()
        .unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        let mut buffers = buffers.into_iter();
        let _input = rw(buffers.next().unwrap());
        let output = rw(buffers.next().unwrap());
        assert!(output.as_pod_slice::<u64>().iter().all(|&x| x == 42));
        println!("inc_u64: {:.1} GB/s", access_bytes as f64 / elapsed / 1e9);
    }
}
