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
    let _ = std::fs::create_dir(target_dir.join("third-party"));

    for file in std::fs::read_dir("../third-party").unwrap() {
        let entry = file.unwrap();
        std::fs::copy(
            entry.path(),
            target_dir.join("third-party").join(entry.file_name()),
        )
        .unwrap();
    }

    println!(
        "cargo:rustc-link-search={}",
        target_dir.join("third-party").display()
    );
}
