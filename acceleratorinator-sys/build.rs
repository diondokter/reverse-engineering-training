use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-link-search=../acceleratorinator/target/release");
    println!("cargo:rerun-if-changed=../acceleratorinator/target/release");

    println!("cargo:rustc-link-lib=static=acceleratorinator");

    let native_libs: &[&str] = if cfg!(windows) {
        &["winusb", "cfgmgr32", "ole32"]
    } else if cfg!(unix) {
        &[]
    } else {
        todo!()
    };

    for nl in native_libs {
        println!("cargo:rustc-link-lib={nl}");
    }

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .use_core()
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_item("cring_.*")
        .allowlist_item("CRING_.*")
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
