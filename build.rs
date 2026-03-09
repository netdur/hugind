use std::env;
use std::path::{Path, PathBuf};

fn find_llama_common_dirs(build_root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Ok(entries) = std::fs::read_dir(build_root) else {
        return dirs;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.starts_with("llama-cpp-ffi-") {
            continue;
        }
        let common_dir = path.join("out").join("build").join("common");
        if common_dir.is_dir() {
            dirs.push(common_dir);
        }
    }

    dirs
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let build_root = out_dir
        .ancestors()
        .nth(2)
        .expect("failed to locate target/*/build directory");
    let common_dirs = find_llama_common_dirs(build_root);

    if common_dirs.is_empty() {
        println!(
            "cargo:warning=llama-cpp-ffi common library directory not found under {}",
            build_root.display()
        );
        return;
    }

    for dir in common_dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    println!("cargo:rustc-link-lib=static=common");
}
