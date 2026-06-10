fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        println!("cargo:rerun-if-changed=res/mpv-icon.ico");
        println!("cargo:rerun-if-changed=res/umpv.rc");

        let out_dir = std::env::var("OUT_DIR").unwrap();
        let res_output = format!("{out_dir}/umpv.res");
        let status = std::process::Command::new("llvm-rc")
            .args(["/fo", &res_output, "res/umpv.rc"])
            .status()
            .expect("failed to run llvm-rc");
        assert!(status.success(), "llvm-rc failed");
        println!("cargo:rustc-link-arg={res_output}");
    }
}
