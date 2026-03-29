fn main() {
    // Tell Cargo to look for libraries in the current folder and the parent folder
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-search=native={}", dir);
    println!("cargo:rustc-link-search=native={}/..", dir);
    
    // Tell Cargo to link `libaudio_engine.so`
    println!("cargo:rustc-link-lib=dylib=audio_engine");
}