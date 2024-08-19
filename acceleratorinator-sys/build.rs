use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-link-search=../acceleratorinator/target/release");

    println!("cargo:rustc-link-lib=acceleratorinator");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .use_core()
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_item("cring_.*")
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}