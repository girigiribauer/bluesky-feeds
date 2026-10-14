use std::path::Path;

fn main() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/build-ncnn.sh");
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rerun-if-env-changed=NCNN_DIR");
    let expected = std::fs::read_to_string(&script)
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("commit=").map(str::to_string))
        })
        .expect("commit= line in scripts/build-ncnn.sh");
    let Ok(dir) = std::env::var("NCNN_DIR") else {
        panic!("NCNN_DIR is not set. Build ncnn with `scripts/build-ncnn.sh <dir>` and set NCNN_DIR=<dir>");
    };
    let marker = Path::new(&dir).join("ncnn-commit.txt");
    println!("cargo:rerun-if-changed={}", marker.display());
    let actual = std::fs::read_to_string(&marker).unwrap_or_default();
    if actual.trim() != expected {
        panic!(
            "ncnn in {dir} is not built from {expected}. Rebuild it with `scripts/build-ncnn.sh {dir}`"
        );
    }
    println!("cargo:rustc-link-search=native={dir}/lib");
    println!("cargo:rustc-link-lib=static=ncnn");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => println!("cargo:rustc-link-lib=dylib=c++"),
        _ => {
            println!("cargo:rustc-link-lib=dylib=stdc++");
            println!("cargo:rustc-link-lib=dylib=pthread");
        }
    }
}
