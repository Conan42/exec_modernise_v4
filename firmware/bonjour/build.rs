use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let linker = manifest_dir.join("linker.ld");
    println!("cargo:rerun-if-changed={}", linker.display());
    println!(
        "cargo:rustc-link-arg-bin=exec-modernise-v4-bonjour=-T{}",
        linker.display()
    );
}
