use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../third-party");

    println!("cargo:rustc-link-lib=acceleratorinator");

    let native_libs: &[&str] = if cfg!(windows) {
        &["winusb", "cfgmgr32", "ole32"]
    } else {
        &[]
    };

    for nl in native_libs {
        println!("cargo:rustc-link-lib={nl}");
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let target_dir = out_dir.ancestors().skip(3).next().unwrap();

    std::fs::copy(
        "../libacceleratorinator.so",
        target_dir.join("libacceleratorinator.so"),
    )
    .unwrap();

    println!("cargo:rustc-link-search={}", target_dir.display());
}
