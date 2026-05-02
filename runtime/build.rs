use std::process::Command;

fn main() {
    Command::new("gcc")
        .args([
            "-fPIC",
            "-shared",
            "-o",
            "../target/debug/libgc.so",
            "-I",
            "src",
            "src/gc.c",
        ])
        .status()
        .unwrap();

    println!("cargo:rerun-if-changed=src/gc.c");
}
