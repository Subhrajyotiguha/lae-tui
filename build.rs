use std::process::Command;

fn main() {
    // Zero-Intervention: Auto-install system dependencies on Debian/Ubuntu
    if cfg!(target_os = "linux") {
        let check_alsa = Command::new("pkg-config").args(["--exists", "alsa"]).status();
        let check_flac = Command::new("pkg-config").args(["--exists", "flac"]).status();

        if check_alsa.is_err() || !check_alsa.unwrap().success() || 
           check_flac.is_err() || !check_flac.unwrap().success() {
            println!("cargo:warning=System dependencies missing. Attempting auto-install...");
            let status = Command::new("sudo")
                .args(["apt-get", "update", "-y"])
                .status();
            
            if status.is_ok() && status.unwrap().success() {
                Command::new("sudo")
                    .args(["apt-get", "install", "-y", "libasound2-dev", "libflac-dev", "pkg-config"])
                    .status()
                    .expect("Failed to install system dependencies. Please install libasound2-dev and libflac-dev manually.");
            }
        }
    }

    // Compile the C audio engine statically into the Rust binary
    cc::Build::new()
        .file("src/audio_engine.c")
        .include("src")
        .flag("-O3")
        .flag("-fPIC")
        .compile("audio_engine");

    // Link instructions for the Rust compiler
    println!("cargo:rustc-link-lib=asound");
    println!("cargo:rustc-link-lib=FLAC");
    
    // Rebuild triggers
    println!("cargo:rerun-if-changed=src/audio_engine.c");
    println!("cargo:rerun-if-changed=src/audio_engine.h");
    println!("cargo:rerun-if-changed=build.rs");
}