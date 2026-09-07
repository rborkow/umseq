use std::{env, path::PathBuf, process::Command};

fn main() {
    if env::var_os("CARGO_FEATURE_CUDA").is_none() {
        return;
    }

    println!("cargo:rerun-if-changed=shim/umgpu_shim.cu");
    println!("cargo:rerun-if-changed=shim/umgpu_shim.h");
    println!("cargo:rerun-if-env-changed=UMGPU_NVCC");
    println!("cargo:rerun-if-env-changed=UMGPU_CUDA_ARCH");
    println!("cargo:rerun-if-env-changed=UMGPU_NVCOMP");
    let nvcc = env::var("UMGPU_NVCC").unwrap_or_else(|_| "/usr/local/cuda/bin/nvcc".into());
    let arch = env::var("UMGPU_CUDA_ARCH").unwrap_or_else(|_| "sm_121".into());
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let object = out.join("umgpu_shim.o");
    let mut command = Command::new(&nvcc);
    command
        .args(["-std=c++17", "-c", "shim/umgpu_shim.cu", "-o"])
        .arg(&object)
        .arg(format!("-arch={arch}"))
        .arg("-I/usr/local/cuda/include/cccl");
    if env::var_os("CARGO_FEATURE_NVCOMP").is_some() {
        let nvcomp = env::var("UMGPU_NVCOMP").unwrap_or_else(|_| {
            let home = env::var("HOME").expect("HOME missing while locating nvCOMP");
            format!("{home}/.local/opt/nvcomp")
        });
        command
            .arg("-DUMGPU_NVCOMP")
            .arg(format!("-I{nvcomp}/include"));
        println!("cargo:rustc-link-search=native={nvcomp}/lib");
        println!("cargo:rustc-link-lib=nvcomp");
    }
    let status = command
        .status()
        .expect("failed to run nvcc; set UMGPU_NVCC to the CUDA compiler");
    assert!(status.success(), "nvcc failed compiling umgpu shim");
    println!("cargo:rerun-if-changed=shim/seed_probe.cu");
    println!("cargo:rerun-if-changed=shim/seed_probe.h");
    println!("cargo:rerun-if-changed=shim/seed_probe_abi.h");
    let probe_object = out.join("seed_probe.o");
    let probe_status = Command::new(&nvcc)
        .args([
            "-std=c++17",
            "-O3",
            "-lineinfo",
            "-c",
            "shim/seed_probe.cu",
            "-o",
        ])
        .arg(&probe_object)
        .arg(format!("-arch={arch}"))
        .arg("-I/usr/local/cuda/include/cccl")
        .status()
        .expect("failed to run nvcc for PROBE");
    assert!(probe_status.success(), "nvcc failed compiling PROBE");
    cc::Build::new()
        .object(&object)
        .object(&probe_object)
        .compile("umgpu_shim");
    println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
    println!("cargo:rustc-link-lib=cudart");
    // CUB/Thrust device algorithms throw std::runtime_error on host-side errors.
    println!("cargo:rustc-link-lib=stdc++");
}
