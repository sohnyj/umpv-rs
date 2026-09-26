use std::env;
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=res/umpv.ico");
    println!("cargo::rerun-if-changed=res/umpv.manifest");
    println!("cargo::rerun-if-changed=res/umpv.rc");

    let output_directory = env::var("OUT_DIR").expect("Cargo should set OUT_DIR for build scripts");
    let resource_path = format!("{output_directory}/umpv.res");
    let status = Command::new("llvm-rc")
        .args(["/fo", &resource_path, "res/umpv.rc"])
        .status()
        .expect("llvm-rc should be installed and on PATH");
    assert!(status.success(), "llvm-rc failed");
    println!("cargo::rustc-link-arg={resource_path}");
}
