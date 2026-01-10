use std::env;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    
    // Only link resources when targeting Windows
    if target.contains("windows") {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let resources_path = PathBuf::from(&manifest_dir).join("resources.o");
        
        if resources_path.exists() {
            println!("cargo:rustc-link-arg={}", resources_path.display());
            println!("cargo:rerun-if-changed=resources.rc");
            println!("cargo:rerun-if-changed=icon.ico");
        } else {
            println!("cargo:warning=resources.o not found. Icon will not be embedded.");
            println!("cargo:warning=Run: x86_64-w64-mingw32-windres resources.rc -O coff -o resources.o");
        }
    }
}
