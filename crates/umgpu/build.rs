use std::{env, path::PathBuf, process::Command};

fn main() {
    if env::var_os("CARGO_FEATURE_CUDA").is_none() {
        return;
    }

    println!("cargo:rerun-if-changed=shim/umgpu_shim.cu");
    println!("cargo:rerun-if-changed=shim/umgpu_shim.h");
    println!("cargo:rerun-if-env-changed=UMGPU_NVCC");
    println!("cargo:rerun-if-env-changed=UMGPU_CUDA_ARCH");
    let nvcc = env::var("UMGPU_NVCC").unwrap_or_else(|_| "/usr/local/cuda/bin/nvcc".into());
    let arch = env::var("UMGPU_CUDA_ARCH").unwrap_or_else(|_| "sm_121".into());
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let object = out.join("umgpu_shim.o");
    let status = Command::new(nvcc)
        .args(["-std=c++17", "-c", "shim/umgpu_shim.cu", "-o"])
        .arg(&object)
        .arg(format!("-arch={arch}"))
        .arg("-I/usr/local/cuda/include/cccl")
        .status()
        .expect("failed to run nvcc; set UMGPU_NVCC to the CUDA compiler");
    assert!(status.success(), "nvcc failed compiling umgpu shim");
    cc::Build::new().object(&object).compile("umgpu_shim");
    println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
    println!("cargo:rustc-link-lib=cudart");
    // CUB/Thrust device algorithms throw std::runtime_error on host-side errors.
    println!("cargo:rustc-link-lib=stdc++");
}
